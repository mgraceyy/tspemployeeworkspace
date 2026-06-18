use axum::{
    extract::{Path, State},
    response::Redirect,
    Form,
};
use minijinja::context;
use serde::Deserialize;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::AppResult;
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    audit::log_action,
    pin_reset::{approve_request, deny_request, list_pending_for_reviewer},
    settings::get_settings,
    timezone::format_time,
};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct ApprovePinResetForm {
    temp_pin: String,
}

#[derive(Deserialize)]
pub struct DenyPinResetForm {
    review_note: String,
}

pub async fn pin_resets_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let is_admin = user.role.is_admin();
    let requests = list_pending_for_reviewer(&state.pool, user.employee_id, is_admin).await?;

    let rows: Vec<_> = requests
        .iter()
        .map(|r| {
            context! {
                id => r.id,
                employee_id => r.employee_id,
                employee_code => r.employee_code.clone(),
                full_name => r.full_name.clone(),
                reason => r.reason.clone().unwrap_or_default(),
                requested_at => format_time(r.requested_at, &settings.timezone),
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "PIN Reset Requests",
        "manager/pin_resets.html",
        context! {
            requests => rows,
            is_admin => is_admin,
        },
    )
    .await
}

pub async fn approve_pin_reset(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(request_id): Path<Uuid>,
    Form(form): Form<ApprovePinResetForm>,
) -> AppResult<Redirect> {
    let is_admin = user.role.is_admin();
    approve_request(
        &state.pool,
        request_id,
        user.employee_id,
        is_admin,
        form.temp_pin.trim(),
    )
    .await?;

    log_action(
        &state.pool,
        user.employee_id,
        "auth.pin_reset_approved",
        &format!("Approved PIN reset request {request_id}"),
    )
    .await?;

    redirect_with_flash(
        &session,
        "/manager/pin-resets",
        "success",
        "PIN reset approved — employee must change PIN on next login",
    )
    .await
}

pub async fn deny_pin_reset(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(request_id): Path<Uuid>,
    Form(form): Form<DenyPinResetForm>,
) -> AppResult<Redirect> {
    let is_admin = user.role.is_admin();
    deny_request(
        &state.pool,
        request_id,
        user.employee_id,
        is_admin,
        &form.review_note,
    )
    .await?;

    log_action(
        &state.pool,
        user.employee_id,
        "auth.pin_reset_denied",
        &format!("Denied PIN reset request {request_id}"),
    )
    .await?;

    redirect_with_flash(
        &session,
        "/manager/pin-resets",
        "success",
        "PIN reset request denied",
    )
    .await
}
