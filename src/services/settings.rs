use sqlx::PgPool;

use crate::error::{AppError, AppResult};
use crate::models::{CompanySettings, PayPeriodType};

pub async fn get_settings(pool: &PgPool) -> AppResult<CompanySettings> {
    let settings = sqlx::query_as::<_, CompanySettings>(
        "SELECT company_name, break_minutes, ot_threshold_minutes, grace_minutes,
                pay_period, timezone, ot_requires_approval
         FROM company_settings
         WHERE id = 1",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(settings)
}

pub async fn update_settings(
    pool: &PgPool,
    break_minutes: i32,
    ot_threshold_minutes: i32,
    grace_minutes: i32,
    pay_period: PayPeriodType,
) -> AppResult<CompanySettings> {
    let settings = sqlx::query_as::<_, CompanySettings>(
        "UPDATE company_settings
         SET break_minutes = $1,
             ot_threshold_minutes = $2,
             grace_minutes = $3,
             pay_period = $4
         WHERE id = 1
         RETURNING company_name, break_minutes, ot_threshold_minutes, grace_minutes,
                   pay_period, timezone, ot_requires_approval",
    )
    .bind(break_minutes)
    .bind(ot_threshold_minutes)
    .bind(grace_minutes)
    .bind(pay_period)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(settings)
}