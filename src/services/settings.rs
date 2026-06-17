use sqlx::PgPool;
use time::Date;

use crate::error::{AppError, AppResult};
use crate::models::{CompanySettings, PayPeriodType};
use crate::services::timezone::validate_timezone;

pub async fn get_settings(pool: &PgPool) -> AppResult<CompanySettings> {
    let settings = sqlx::query_as::<_, CompanySettings>(
        "SELECT company_name, break_minutes, ot_threshold_minutes, grace_minutes,
                pay_period, pay_period_anchor, timezone, ot_requires_approval
         FROM company_settings
         WHERE id = 1",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(settings)
}

pub struct SettingsUpdate<'a> {
    pub company_name: &'a str,
    pub timezone: &'a str,
    pub break_minutes: i32,
    pub ot_threshold_minutes: i32,
    pub grace_minutes: i32,
    pub pay_period: PayPeriodType,
    pub pay_period_anchor: Date,
    pub ot_requires_approval: bool,
}

pub async fn update_settings(
    pool: &PgPool,
    update: &SettingsUpdate<'_>,
) -> AppResult<CompanySettings> {
    if update.company_name.trim().is_empty() {
        return Err(AppError::bad_request("Company name is required"));
    }
    if update.timezone.trim().is_empty() {
        return Err(AppError::bad_request("Timezone is required"));
    }
    validate_timezone(update.timezone)?;

    let settings = sqlx::query_as::<_, CompanySettings>(
        "UPDATE company_settings
         SET company_name = $1,
             timezone = $2,
             break_minutes = $3,
             ot_threshold_minutes = $4,
             grace_minutes = $5,
             pay_period = $6,
             pay_period_anchor = $7,
             ot_requires_approval = $8
         WHERE id = 1
         RETURNING company_name, break_minutes, ot_threshold_minutes, grace_minutes,
                   pay_period, pay_period_anchor, timezone, ot_requires_approval",
    )
    .bind(update.company_name.trim())
    .bind(update.timezone.trim())
    .bind(update.break_minutes)
    .bind(update.ot_threshold_minutes)
    .bind(update.grace_minutes)
    .bind(update.pay_period)
    .bind(update.pay_period_anchor)
    .bind(update.ot_requires_approval)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(settings)
}
