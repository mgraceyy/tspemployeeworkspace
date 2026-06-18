use axum::{extract::State, response::Redirect, Form};
use minijinja::context;
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::post_limiter::ClientIp;
use crate::auth::{clear_session, set_session, sync_session_with_db, verify_pin, UserSession};
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, PageOrRedirect};
use crate::services::employees::{change_own_pin, find_by_code, validate_pin};
use crate::services::settings::get_settings;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct LoginForm {
    employee_code: String,
    pin: String,
}

#[derive(Deserialize)]
pub struct ChangePinForm {
    current_pin: Option<String>,
    new_pin: String,
    confirm_pin: String,
}

pub async fn login_page(
    State(state): State<AppState>,
    session: Session,
) -> AppResult<PageOrRedirect> {
    if let Ok(user) = sync_session_with_db(&state.pool, &session).await {
        let target = if user.must_change_pin {
            "/change-pin"
        } else {
            "/"
        };
        return Ok(PageOrRedirect::Redirect(Redirect::to(target)));
    }

    let settings = get_settings(&state.pool).await?;
    let page = render_page(
        &state,
        &session,
        None,
        &settings.company_name,
        "Login",
        "login.html",
        context! {
            error => None::<String>,
        },
    )
    .await?;
    Ok(PageOrRedirect::Page(page))
}

pub async fn login_submit(
    State(state): State<AppState>,
    session: Session,
    ClientIp(client_ip): ClientIp,
    Form(form): Form<LoginForm>,
) -> AppResult<PageOrRedirect> {
    let settings = get_settings(&state.pool).await?;
    let employee_code = form.employee_code.trim().to_uppercase();
    let pin = form.pin.trim();

    if state.login_limiter.is_locked_ip(&client_ip).await? {
        return login_error_page(
            &state,
            &session,
            &settings.company_name,
            "Too many login attempts from this address. Try again in 15 minutes.",
        )
        .await;
    }

    if state
        .login_limiter
        .is_locked_account(&employee_code)
        .await?
    {
        return login_error_page(
            &state,
            &session,
            &settings.company_name,
            "Too many failed login attempts. Try again in 15 minutes.",
        )
        .await;
    }

    let Some(employee) = find_by_code(&state.pool, &employee_code).await? else {
        state
            .login_limiter
            .record_failure_account(&employee_code)
            .await?;
        state.login_limiter.record_failure_ip(&client_ip).await?;
        return login_error_page(
            &state,
            &session,
            &settings.company_name,
            "Invalid employee code or PIN",
        )
        .await;
    };

    if !verify_pin(pin, &employee.pin_hash)? {
        state
            .login_limiter
            .record_failure_account(&employee_code)
            .await?;
        state.login_limiter.record_failure_ip(&client_ip).await?;
        return login_error_page(
            &state,
            &session,
            &settings.company_name,
            "Invalid employee code or PIN",
        )
        .await;
    }

    state.login_limiter.clear_account(&employee_code).await?;

    session
        .flush()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    crate::auth::csrf::get_or_create_token(&session).await?;

    set_session(
        &session,
        UserSession {
            employee_id: employee.id,
            employee_code: employee.employee_code,
            full_name: employee.full_name,
            role: employee.role,
            must_change_pin: employee.must_change_pin,
            session_version: employee.session_version,
        },
    )
    .await?;

    let target = if employee.must_change_pin {
        "/change-pin"
    } else {
        "/"
    };
    Ok(PageOrRedirect::Redirect(Redirect::to(target)))
}

pub async fn change_pin_page(
    State(state): State<AppState>,
    session: Session,
) -> AppResult<PageOrRedirect> {
    let user = sync_session_with_db(&state.pool, &session).await?;
    let settings = get_settings(&state.pool).await?;
    let page = render_page(
        &state,
        &session,
        Some(user.clone()),
        &settings.company_name,
        "Change PIN",
        "change_pin.html",
        context! {
            error => None::<String>,
            forced => user.must_change_pin,
        },
    )
    .await?;
    Ok(PageOrRedirect::Page(page))
}

pub async fn change_pin_submit(
    State(state): State<AppState>,
    session: Session,
    ClientIp(client_ip): ClientIp,
    Form(form): Form<ChangePinForm>,
) -> AppResult<PageOrRedirect> {
    let user = sync_session_with_db(&state.pool, &session).await?;
    let settings = get_settings(&state.pool).await?;
    let new_pin = form.new_pin.trim();
    let confirm_pin = form.confirm_pin.trim();

    if state
        .login_limiter
        .is_locked_pin_change_ip(&client_ip)
        .await?
    {
        return change_pin_error_page(
            &state,
            &session,
            &settings.company_name,
            &user,
            "Too many PIN change attempts from this address. Try again in 15 minutes.",
        )
        .await;
    }

    if state
        .login_limiter
        .is_locked_pin_change_account(&user.employee_code)
        .await?
    {
        return change_pin_error_page(
            &state,
            &session,
            &settings.company_name,
            &user,
            "Too many failed PIN change attempts. Try again in 15 minutes.",
        )
        .await;
    }

    if new_pin != confirm_pin {
        return change_pin_error_page(
            &state,
            &session,
            &settings.company_name,
            &user,
            "New PIN and confirmation do not match",
        )
        .await;
    }

    if let Err(e) = validate_pin(new_pin) {
        return change_pin_error_page(
            &state,
            &session,
            &settings.company_name,
            &user,
            &e.to_string(),
        )
        .await;
    }

    if !user.must_change_pin {
        let Some(current) = form
            .current_pin
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return change_pin_error_page(
                &state,
                &session,
                &settings.company_name,
                &user,
                "Current PIN is required",
            )
            .await;
        };
        let employee = find_by_code(&state.pool, &user.employee_code)
            .await?
            .ok_or(AppError::Unauthorized)?;
        if !verify_pin(current, &employee.pin_hash)? {
            state
                .login_limiter
                .record_pin_change_failure_account(&user.employee_code)
                .await?;
            state
                .login_limiter
                .record_pin_change_failure_ip(&client_ip)
                .await?;
            return change_pin_error_page(
                &state,
                &session,
                &settings.company_name,
                &user,
                "Current PIN is incorrect",
            )
            .await;
        }
    }

    change_own_pin(&state.pool, user.employee_id, new_pin).await?;

    state
        .login_limiter
        .clear_pin_change_account(&user.employee_code)
        .await?;

    set_session(
        &session,
        UserSession {
            must_change_pin: false,
            ..user
        },
    )
    .await?;

    let redirect =
        redirect_with_flash(&session, "/", "success", "PIN updated successfully").await?;
    Ok(PageOrRedirect::Redirect(redirect))
}

pub async fn logout(session: Session) -> AppResult<Redirect> {
    clear_session(&session).await?;
    Ok(Redirect::to("/login"))
}

#[derive(Deserialize)]
pub struct PinResetRequestForm {
    employee_code: String,
    reason: Option<String>,
}

pub async fn pin_reset_request_page(
    State(state): State<AppState>,
    session: Session,
) -> AppResult<PageOrRedirect> {
    if sync_session_with_db(&state.pool, &session).await.is_ok() {
        return Ok(PageOrRedirect::Redirect(Redirect::to("/me/profile")));
    }

    let settings = get_settings(&state.pool).await?;
    let page = render_page(
        &state,
        &session,
        None,
        &settings.company_name,
        "Request PIN Reset",
        "pin_reset_request.html",
        context! {
            error => None::<String>,
            success => None::<String>,
        },
    )
    .await?;
    Ok(PageOrRedirect::Page(page))
}

pub async fn pin_reset_request_submit(
    State(state): State<AppState>,
    session: Session,
    ClientIp(client_ip): ClientIp,
    Form(form): Form<PinResetRequestForm>,
) -> AppResult<PageOrRedirect> {
    let settings = get_settings(&state.pool).await?;

    if state.post_limiter.is_limited(&client_ip).await? {
        return pin_reset_request_feedback(
            &state,
            &session,
            &settings.company_name,
            None,
            Some("Too many requests. Try again in a minute."),
        )
        .await;
    }

    crate::services::pin_reset::create_request_by_code(
        &state.pool,
        &form.employee_code,
        form.reason.as_deref(),
    )
    .await?;

    pin_reset_request_feedback(
        &state,
        &session,
        &settings.company_name,
        Some("If your employee code is valid, your manager or admin will review the request."),
        None,
    )
    .await
}

async fn pin_reset_request_feedback(
    state: &AppState,
    session: &Session,
    company_name: &str,
    success: Option<&str>,
    error: Option<&str>,
) -> AppResult<PageOrRedirect> {
    render_page(
        state,
        session,
        None,
        company_name,
        "Request PIN Reset",
        "pin_reset_request.html",
        context! {
            error => error.map(str::to_string),
            success => success.map(str::to_string),
        },
    )
    .await
    .map(PageOrRedirect::Page)
}

async fn login_error_page(
    state: &AppState,
    session: &Session,
    company_name: &str,
    message: &str,
) -> AppResult<PageOrRedirect> {
    render_page(
        state,
        session,
        None,
        company_name,
        "Login",
        "login.html",
        context! {
            error => Some(message.to_string()),
        },
    )
    .await
    .map(PageOrRedirect::Page)
}

async fn change_pin_error_page(
    state: &AppState,
    session: &Session,
    company_name: &str,
    user: &UserSession,
    message: &str,
) -> AppResult<PageOrRedirect> {
    render_page(
        state,
        session,
        Some(user.clone()),
        company_name,
        "Change PIN",
        "change_pin.html",
        context! {
            error => Some(message.to_string()),
            forced => user.must_change_pin,
        },
    )
    .await
    .map(PageOrRedirect::Page)
}
