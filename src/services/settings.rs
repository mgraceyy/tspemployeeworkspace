use sqlx::PgPool;
use time::Date;

use crate::error::{AppError, AppResult};
use crate::models::{CompanySettings, PayPeriodType};
use crate::services::timezone::validate_timezone;

pub async fn get_settings(pool: &PgPool) -> AppResult<CompanySettings> {
    let settings = sqlx::query_as::<_, CompanySettings>(
        "SELECT company_name, break_minutes, ot_threshold_minutes, grace_minutes,
                pay_period, pay_period_anchor, timezone, ot_requires_approval,
                journal_salary_expense_account, journal_net_payable_account,
                journal_salary_expense_label, journal_net_payable_label
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
    pub journal_salary_expense_account: &'a str,
    pub journal_net_payable_account: &'a str,
    pub journal_salary_expense_label: &'a str,
    pub journal_net_payable_label: &'a str,
}

fn require_journal_field<'a>(value: &'a str, label: &str) -> AppResult<&'a str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::bad_request(format!("{label} is required")));
    }
    Ok(trimmed)
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
    let salary_account = require_journal_field(
        update.journal_salary_expense_account,
        "Salary expense account",
    )?;
    let payable_account = require_journal_field(
        update.journal_net_payable_account,
        "Net pay payable account",
    )?;
    let salary_label =
        require_journal_field(update.journal_salary_expense_label, "Salary expense label")?;
    let payable_label =
        require_journal_field(update.journal_net_payable_label, "Net pay payable label")?;

    let settings = sqlx::query_as::<_, CompanySettings>(
        "UPDATE company_settings
         SET company_name = $1,
             timezone = $2,
             break_minutes = $3,
             ot_threshold_minutes = $4,
             grace_minutes = $5,
             pay_period = $6,
             pay_period_anchor = $7,
             ot_requires_approval = $8,
             journal_salary_expense_account = $9,
             journal_net_payable_account = $10,
             journal_salary_expense_label = $11,
             journal_net_payable_label = $12
         WHERE id = 1
         RETURNING company_name, break_minutes, ot_threshold_minutes, grace_minutes,
                   pay_period, pay_period_anchor, timezone, ot_requires_approval,
                   journal_salary_expense_account, journal_net_payable_account,
                   journal_salary_expense_label, journal_net_payable_label",
    )
    .bind(update.company_name.trim())
    .bind(update.timezone.trim())
    .bind(update.break_minutes)
    .bind(update.ot_threshold_minutes)
    .bind(update.grace_minutes)
    .bind(update.pay_period)
    .bind(update.pay_period_anchor)
    .bind(update.ot_requires_approval)
    .bind(salary_account)
    .bind(payable_account)
    .bind(salary_label)
    .bind(payable_label)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(settings)
}
