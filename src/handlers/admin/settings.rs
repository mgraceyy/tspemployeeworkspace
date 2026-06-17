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
