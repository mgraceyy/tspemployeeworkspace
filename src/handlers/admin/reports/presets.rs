use axum::{
    extract::{Path, State},
    response::Redirect,
    Form,
};
use serde::Deserialize;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::AppResult;
use crate::handlers::flash::redirect_with_flash;
use crate::models::UserRole;
use crate::services::{
    audit::log_action,
    payroll_controls::{create_report_preset, delete_report_preset},
};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct SaveReportPresetForm {
    preset_name: String,
    department: Option<String>,
    role: Option<String>,
    employee_id: Option<Uuid>,
}

pub async fn save_report_preset_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<SaveReportPresetForm>,
) -> AppResult<Redirect> {
    let role = form.role.as_deref().and_then(|value| match value {
        "employee" => Some(UserRole::Employee),
        "manager" => Some(UserRole::Manager),
        "admin" => Some(UserRole::Admin),
        _ => None,
    });
    let department = form
        .department
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let created = create_report_preset(
        &state.pool,
        &form.preset_name,
        department,
        role,
        form.employee_id,
        user.employee_id,
    )
    .await?;

    log_action(
        &state.pool,
        user.employee_id,
        "reports.preset_saved",
        &format!("Saved report preset \"{}\"", created.name),
    )
    .await?;

    redirect_with_flash(
        &session,
        "/admin/reports",
        "success",
        &format!("Saved preset \"{}\"", created.name),
    )
    .await
}

pub async fn delete_report_preset_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(preset_id): Path<Uuid>,
) -> AppResult<Redirect> {
    delete_report_preset(&state.pool, preset_id).await?;

    log_action(
        &state.pool,
        user.employee_id,
        "reports.preset_deleted",
        "Deleted report preset",
    )
    .await?;

    redirect_with_flash(&session, "/admin/reports", "success", "Preset deleted").await
}
