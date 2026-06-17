use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{AttendanceStatus, CompanySettings, ShiftTemplate};
use crate::services::team::assert_can_manage;

pub fn evaluate_attendance(
    clock_in: OffsetDateTime,
    clock_out: Option<OffsetDateTime>,
    shift: Option<&ShiftTemplate>,
    settings: &CompanySettings,
    work_date: Date,
) -> AttendanceStatus {
    let Some(shift) = shift else {
        return AttendanceStatus::OnTime;
    };

    let shift_start = super::timezone::combine_date_time(work_date, shift.start_time);
    let shift_end = super::timezone::combine_date_time(work_date, shift.end_time);
    let grace_limit = shift_start + time::Duration::minutes(settings.grace_minutes as i64);

    if clock_in > grace_limit {
        return AttendanceStatus::Late;
    }

    if let Some(clock_out) = clock_out {
        let expected_minutes = (shift_end - shift_start).whole_minutes() as i32;
        let worked_minutes = (clock_out - clock_in).whole_minutes() as i32;
        if worked_minutes + 30 < expected_minutes {
            return AttendanceStatus::Partial;
        }
    }

    AttendanceStatus::OnTime
}

pub async fn get_shift_for_date(
    pool: &PgPool,
    employee_id: Uuid,
    work_date: Date,
) -> AppResult<Option<ShiftTemplate>> {
    let day_of_week = work_date.weekday().number_days_from_sunday() as i16;
    let shift = sqlx::query_as::<_, ShiftTemplate>(
        "SELECT id, employee_id, day_of_week, start_time, end_time
         FROM shift_templates
         WHERE employee_id = $1 AND day_of_week = $2",
    )
    .bind(employee_id)
    .bind(day_of_week)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(shift)
}

pub async fn mark_no_show_for_employee(
    pool: &PgPool,
    employee_id: Uuid,
    work_date: Date,
    editor_id: Uuid,
    is_admin: bool,
    manager_id: Uuid,
) -> AppResult<()> {
    assert_can_manage(pool, manager_id, employee_id, is_admin).await?;

    let existing = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM time_entries WHERE employee_id = $1 AND work_date = $2",
    )
    .bind(employee_id)
    .bind(work_date)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if let Some(entry_id) = existing {
        sqlx::query(
            "UPDATE time_entries SET attendance = 'no_show' WHERE id = $1",
        )
        .bind(entry_id)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    } else {
        sqlx::query(
            "INSERT INTO time_entries (employee_id, work_date, attendance)
             VALUES ($1, $2, 'no_show')",
        )
        .bind(employee_id)
        .bind(work_date)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    }

    tracing::info!(
        employee_id = %employee_id,
        work_date = %work_date,
        editor_id = %editor_id,
        "marked no-show"
    );
    Ok(())
}