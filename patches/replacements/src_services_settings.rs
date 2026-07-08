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
                journal_salary_expense_label, journal_net_payable_label,
                auto_deduct_sss, auto_deduct_phic, auto_deduct_hdmf, auto_deduct_wht,
                premium_rest_day, premium_holiday, premium_night_diff,
                default_vacation_days, default_sick_days
         FROM company_settings
         WHERE id = 1",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(settings)
}

pub fn settings_update_from<'a>(settings: &'a CompanySettings) -> SettingsUpdate<'a> {
    SettingsUpdate {
        company_name: &settings.company_name,
        timezone: &settings.timezone,
        break_minutes: settings.break_minutes,
        ot_threshold_minutes: settings.ot_threshold_minutes,
        grace_minutes: settings.grace_minutes,
        pay_period: settings.pay_period,
        pay_period_anchor: settings.pay_period_anchor,
        ot_requires_approval: settings.ot_requires_approval,
        journal_salary_expense_account: &settings.journal_salary_expense_account,
        journal_net_payable_account: &settings.journal_net_payable_account,
        journal_salary_expense_label: &settings.journal_salary_expense_label,
        journal_net_payable_label: &settings.journal_net_payable_label,
        auto_deduct_sss: settings.auto_deduct_sss,
        auto_deduct_phic: settings.auto_deduct_phic,
        auto_deduct_hdmf: settings.auto_deduct_hdmf,
        auto_deduct_wht: settings.auto_deduct_wht,
        premium_rest_day: settings.premium_rest_day,
        premium_holiday: settings.premium_holiday,
        premium_night_diff: settings.premium_night_diff,
        default_vacation_days: settings.default_vacation_days,
        default_sick_days: settings.default_sick_days,
    }
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
    pub auto_deduct_sss: bool,
    pub auto_deduct_phic: bool,
    pub auto_deduct_hdmf: bool,
    pub auto_deduct_wht: bool,
    pub premium_rest_day: bool,
    pub premium_holiday: bool,
    pub premium_night_diff: bool,
    pub default_vacation_days: i32,
    pub default_sick_days: i32,
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
    if update.default_vacation_days < 0 {
        return Err(AppError::bad_request(
            "Default vacation days cannot be negative",
        ));
    }
    if update.default_sick_days < 0 {
        return Err(AppError::bad_request("Default sick days cannot be negative"));
    }
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
             journal_net_payable_label = $12,
             auto_deduct_sss = $13,
             auto_deduct_phic = $14,
             auto_deduct_hdmf = $15,
             auto_deduct_wht = $16,
             premium_rest_day = $17,
             premium_holiday = $18,
             premium_night_diff = $19,
             default_vacation_days = $20,
             default_sick_days = $21
         WHERE id = 1
         RETURNING company_name, break_minutes, ot_threshold_minutes, grace_minutes,
                   pay_period, pay_period_anchor, timezone, ot_requires_approval,
                   journal_salary_expense_account, journal_net_payable_account,
                   journal_salary_expense_label, journal_net_payable_label,
                   auto_deduct_sss, auto_deduct_phic, auto_deduct_hdmf, auto_deduct_wht,
                   premium_rest_day, premium_holiday, premium_night_diff,
                   default_vacation_days, default_sick_days",
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
    .bind(update.auto_deduct_sss)
    .bind(update.auto_deduct_phic)
    .bind(update.auto_deduct_hdmf)
    .bind(update.auto_deduct_wht)
    .bind(update.premium_rest_day)
    .bind(update.premium_holiday)
    .bind(update.premium_night_diff)
    .bind(update.default_vacation_days)
    .bind(update.default_sick_days)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(settings)
}