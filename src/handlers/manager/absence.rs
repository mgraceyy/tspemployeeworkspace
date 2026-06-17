use axum::{extract::State, response::Redirect, Form};
use serde::Deserialize;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash;
use crate::models::AttendanceStatus;
use crate::services::{
    attendance::mark_absence_for_employee,
    audit::log_action,
    employees::find_by_id,
    settings::get_settings,
    timezone::{company_date_now, format_date},
};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct AbsenceForm {
    employee_id: Uuid,
    absence_type: String,
}

pub async fn mark_absence(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<AbsenceForm>,
) -> AppResult<Redirect> {
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let status = match form.absence_type.as_str() {
        "sick_leave" => AttendanceStatus::SickLeave,
        "vacation" => AttendanceStatus::Vacation,
        "official_leave" => AttendanceStatus::OfficialLeave,
        "offset" => AttendanceStatus::Offset,
        _ => AttendanceStatus::NoShow,
    };

    mark_absence_for_employee(
        &state.pool,
        form.employee_id,
        today,
        status,
        user.employee_id,
        user.role.is_admin(),
        user.employee_id,
    )
    .await?;

    let employee = find_by_id(&state.pool, form.employee_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let label = match status {
        AttendanceStatus::SickLeave => "sick leave",
        AttendanceStatus::Vacation => "vacation",
        AttendanceStatus::OfficialLeave => "official leave",
        AttendanceStatus::Offset => "offset",
        _ => "no-show",
    };

    log_action(
        &state.pool,
        user.employee_id,
        "attendance.marked",
        &format!(
            "Marked {} ({}) as {} on {}",
            employee.full_name,
            employee.employee_code,
            label,
            format_date(today)
        ),
    )
    .await?;

    redirect_with_flash(
        &session,
        "/manager",
        "success",
        &format!("Marked as {label}"),
    )
    .await
}
