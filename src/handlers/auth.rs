use axum::{
    extract::State,
    response::Redirect,
    Form,
};
use minijinja::context;
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{clear_session, get_session, set_session, verify_pin, UserSession};
use crate::error::{AppError, AppResult};
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

pub async fn login_page(State(state): State<AppState>, session: Session) -> AppResult<PageOrRedirect> {
    if let Ok(user) = get_session(&session).await {
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
        None,
        &settings.company_name,
        "Login",
        "login.html",
        context! {
            error => None::<String>,
        }
        .into(),
    )
    .await?;
    Ok(PageOrRedirect::Page(page))
}

pub async fn login_submit(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<LoginForm>,
) -> AppResult<PageOrRedirect> {
    let settings = get_settings(&state.pool).await?;
    let employee_code = form.employee_code.trim().to_uppercase();
    let pin = form.pin.trim();

    if state.login_limiter.is_locked(&employee_code) {
        return login_error_page(
            &state,
            &settings.company_name,
            "Too many failed attempts. Try again in 15 minutes.",
        )
        .await;
    }

    let Some(employee) = find_by_code(&state.pool, &employee_code).await? else {
        state.login_limiter.record_failure(&employee_code);
        return login_error_page(
            &state,
            &settings.company_name,
            "Invalid employee code or PIN",
        )
        .await;
    };

    if !verify_pin(pin, &employee.pin_hash)? {
        state.login_limiter.record_failure(&employee_code);
        return login_error_page(
            &state,
            &settings.company_name,
            "Invalid employee code or PIN",
        )
        .await;
    }

    state.login_limiter.clear(&employee_code);

    set_session(
        &session,
        UserSession {
            employee_id: employee.id,
            employee_code: employee.employee_code,
            full_name: employee.full_name,
            role: employee.role,
            must_change_pin: employee.must_change_pin,
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

pub async fn change_pin_page(State(state): State<AppState>, session: Session) -> AppResult<PageOrRedirect> {
    let user = get_session(&session).await?;
    if !user.must_change_pin {
        return Ok(PageOrRedirect::Redirect(Redirect::to("/")));
    }

    let settings = get_settings(&state.pool).await?;
    let page = render_page(
        &state,
        Some(user),
        &settings.company_name,
        "Change PIN",
        "change_pin.html",
        context! {
            error => None::<String>,
            forced => true,
        }
        .into(),
    )
    .await?;
    Ok(PageOrRedirect::Page(page))
}

pub async fn change_pin_submit(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<ChangePinForm>,
) -> AppResult<PageOrRedirect> {
    let user = get_session(&session).await?;
    let settings = get_settings(&state.pool).await?;
    let new_pin = form.new_pin.trim();
    let confirm_pin = form.confirm_pin.trim();

    if new_pin != confirm_pin {
        return change_pin_error_page(
            &state,
            &settings.company_name,
            &user,
            "New PIN and confirmation do not match",
        )
        .await;
    }

    if let Err(e) = validate_pin(new_pin) {
        return change_pin_error_page(
            &state,
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
            return change_pin_error_page(
                &state,
                &settings.company_name,
                &user,
                "Current PIN is incorrect",
            )
            .await;
        }
    }

    change_own_pin(&state.pool, user.employee_id, new_pin).await?;

    set_session(
        &session,
        UserSession {
            must_change_pin: false,
            ..user
        },
    )
    .await?;

    Ok(PageOrRedirect::Redirect(Redirect::to("/")))
}

pub async fn logout(session: Session) -> AppResult<Redirect> {
    clear_session(&session).await?;
    Ok(Redirect::to("/login"))
}

async fn login_error_page(
    state: &AppState,
    company_name: &str,
    message: &str,
) -> AppResult<PageOrRedirect> {
    render_page(
        state,
        None,
        company_name,
        "Login",
        "login.html",
        context! {
            error => Some(message.to_string()),
        }
        .into(),
    )
    .await
    .map(PageOrRedirect::Page)
}

async fn change_pin_error_page(
    state: &AppState,
    company_name: &str,
    user: &UserSession,
    message: &str,
) -> AppResult<PageOrRedirect> {
    render_page(
        state,
        Some(user.clone()),
        company_name,
        "Change PIN",
        "change_pin.html",
        context! {
            error => Some(message.to_string()),
            forced => user.must_change_pin,
        }
        .into(),
    )
    .await
    .map(PageOrRedirect::Page)
}