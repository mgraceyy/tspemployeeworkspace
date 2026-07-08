use axum::{extract::State, response::Redirect, Form};
use minijinja::context;
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, HtmlPage};
use crate::models::PayPeriodType;
use crate::services::{
    audit::log_action,
    settings::{get_settings, update_settings, SettingsUpdate},
    timezone::{format_date, parse_date},
};
use crate::state::AppState;

pub async fn settings_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;

    render_page(
        &state,
        &session,
        Some(user.clone()),
        &settings.company_name,
        "Company Settings",
        "admin/settings.html",
        context! {
            settings => context! {
                company_name => settings.company_name,
                timezone => settings.timezone,
                break_minutes => settings.break_minutes,
                ot_threshold_minutes => settings.ot_threshold_minutes,
                grace_minutes => settings.grace_minutes,
                pay_period => settings.pay_period,
                pay_period_anchor => format_date(settings.pay_period_anchor),
                ot_requires_approval => settings.ot_requires_approval,
                auto_deduct_sss => settings.auto_deduct_sss,
                auto_deduct_phic => settings.auto_deduct_phic,
                auto_deduct_hdmf => settings.auto_deduct_hdmf,
                auto_deduct_wht => settings.auto_deduct_wht,
                premium_rest_day => settings.premium_rest_day,
                premium_holiday => settings.premium_holiday,
                premium_night_diff => settings.premium_night_diff,
                default_vacation_days => settings.default_vacation_days,
                default_sick_days => settings.default_sick_days,
                journal_salary_expense_account => settings.journal_salary_expense_account,
                journal_net_payable_account => settings.journal_net_payable_account,
                journal_salary_expense_label => settings.journal_salary_expense_label,
                journal_net_payable_label => settings.journal_net_payable_label,
            },
            message => None::<String>,
        },
    )
    .await
}

#[derive(Deserialize)]
pub struct SettingsForm {
    company_name: String,
    timezone: String,
    break_minutes: i32,
    ot_threshold_minutes: i32,
    grace_minutes: i32,
    pay_period: String,
    pay_period_anchor: String,
    ot_requires_approval: Option<String>,
    auto_deduct_sss: Option<String>,
    auto_deduct_phic: Option<String>,
    auto_deduct_hdmf: Option<String>,
    auto_deduct_wht: Option<String>,
    premium_rest_day: Option<String>,
    premium_holiday: Option<String>,
    premium_night_diff: Option<String>,
    default_vacation_days: i32,
    default_sick_days: i32,
    journal_salary_expense_account: String,
    journal_net_payable_account: String,
    journal_salary_expense_label: String,
    journal_net_payable_label: String,
}

pub async fn save_settings(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<SettingsForm>,
) -> AppResult<Redirect> {
    let pay_period = match form.pay_period.as_str() {
        "weekly" => PayPeriodType::Weekly,
        "biweekly" => PayPeriodType::Biweekly,
        "monthly" => PayPeriodType::Monthly,
        _ => PayPeriodType::Semimonthly,
    };

    let pay_period_anchor = parse_date(&form.pay_period_anchor).map_err(AppError::bad_request)?;

    update_settings(
        &state.pool,
        &SettingsUpdate {
            company_name: &form.company_name,
            timezone: &form.timezone,
            break_minutes: form.break_minutes,
            ot_threshold_minutes: form.ot_threshold_minutes,
            grace_minutes: form.grace_minutes,
            pay_period,
            pay_period_anchor,
            ot_requires_approval: form.ot_requires_approval.is_some(),
            auto_deduct_sss: form.auto_deduct_sss.is_some(),
            auto_deduct_phic: form.auto_deduct_phic.is_some(),
            auto_deduct_hdmf: form.auto_deduct_hdmf.is_some(),
            auto_deduct_wht: form.auto_deduct_wht.is_some(),
            premium_rest_day: form.premium_rest_day.is_some(),
            premium_holiday: form.premium_holiday.is_some(),
            premium_night_diff: form.premium_night_diff.is_some(),
            default_vacation_days: form.default_vacation_days,
            default_sick_days: form.default_sick_days,
            journal_salary_expense_account: &form.journal_salary_expense_account,
            journal_net_payable_account: &form.journal_net_payable_account,
            journal_salary_expense_label: &form.journal_salary_expense_label,
            journal_net_payable_label: &form.journal_net_payable_label,
        },
    )
    .await?;

    log_action(
        &state.pool,
        user.employee_id,
        "settings.updated",
        &format!("Updated company settings for {}", form.company_name.trim()),
    )
    .await?;

    redirect_with_flash(&session, "/admin/settings", "success", "Settings saved").await
}