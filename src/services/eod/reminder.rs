use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::services::settings::get_settings;
use crate::services::timezone::company_date_now;

pub async fn clocked_in_on_date(
    pool: &PgPool,
    employee_id: Uuid,
    work_date: Date,
) -> AppResult<bool> {
    let clocked: Option<bool> = sqlx::query_scalar(
        "SELECT clock_in IS NOT NULL
         FROM time_entries
         WHERE employee_id = $1 AND work_date = $2",
    )
    .bind(employee_id)
    .bind(work_date)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(clocked.unwrap_or(false))
}

pub async fn needs_eod_reminder(pool: &PgPool, employee_id: Uuid) -> AppResult<bool> {
    let settings = get_settings(pool).await?;
    let today = company_date_now(&settings)?;
    if crate::services::holidays::is_holiday(pool, today).await? {
        return Ok(false);
    }
    if !clocked_in_on_date(pool, employee_id, today).await? {
        return Ok(false);
    }
    let submitted: Option<bool> = sqlx::query_scalar(
        "SELECT status = 'submitted'
         FROM eod_reports
         WHERE employee_id = $1 AND report_date = $2",
    )
    .bind(employee_id)
    .bind(today)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(!submitted.unwrap_or(false))
}
