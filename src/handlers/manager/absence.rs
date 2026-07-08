use axum::{extract::State, response::Redirect, Form};
use serde::Deserialize;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash_from_result;
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
        "lwop" => AttendanceStatus::Lwop,
        _ => AttendanceStatus::NoShow,
    };

    let label = match status {
        AttendanceStatus::SickLeave => "sick leave",
        AttendanceStatus::Vacation => "vacation",
        AttendanceStatus::OfficialLeave => "official leave",
        AttendanceStatus::Offset => "offset",
        AttendanceStatus::Lwop => "LWOP",
        _ => "no-show",
    };

    let result: AppResult<()> = async {
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
        Ok(())
    }
    .await;

    redirect_with_flash_from_result(
        &session,
        "/manager",
        &format!("Marked as {label}"),
        result,
    )
    .await
}
