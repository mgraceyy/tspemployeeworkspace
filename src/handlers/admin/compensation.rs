use axum::{
    extract::{Path, State},
    response::Redirect,
    Form,
};
use minijinja::context;
use serde::Deserialize;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    audit::log_action,
    compensation::{format_salary_cents, get_compensation, parse_salary_to_cents, upsert_profile},
    employees::find_by_id,
    settings::get_settings,
    timezone::{format_date, parse_date},
};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CompensationForm {
    monthly_salary: String,
    ot_rate_percent: Option<i32>,
    effective_from: String,
}

pub async fn compensation_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let employee = find_by_id(&state.pool, employee_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let profile = get_compensation(&state.pool, employee_id).await?;

    let (monthly_salary, ot_rate_percent, effective_from) = if let Some(ref p) = profile {
        (
            format_salary_cents(p.monthly_salary_cents),
            p.ot_rate_percent,
            format_date(p.effective_from),
        )
    } else {
        ("0.00".to_string(), 132, String::new())
    };

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Employee Compensation",
        "admin/compensation.html",
        context! {
            employee_id => employee_id,
            employee_code => employee.employee_code,
            full_name => employee.full_name,
            has_profile => profile.is_some(),
            monthly_salary => monthly_salary,
            ot_rate_percent => ot_rate_percent,
            effective_from => effective_from,
            default_ot_rate => 132,
            working_days => crate::services::payroll::MONTHLY_WORKING_DAYS,
        },
    )
    .await
}

pub async fn save_compensation_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
    Form(form): Form<CompensationForm>,
) -> AppResult<Redirect> {
    let employee = find_by_id(&state.pool, employee_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let monthly_salary_cents = parse_salary_to_cents(&form.monthly_salary)?;
    let ot_rate_percent = form.ot_rate_percent.unwrap_or(132);
    let effective_from = parse_date(&form.effective_from).map_err(AppError::bad_request)?;

    upsert_profile(
        &state.pool,
        employee_id,
        monthly_salary_cents,
        ot_rate_percent,
        effective_from,
        user.employee_id,
    )
    .await?;

    log_action(
        &state.pool,
        user.employee_id,
        "compensation.updated",
        &format!(
            "Set compensation for {} ({}): PHP {} / month, OT {}%, effective {}",
            employee.full_name,
            employee.employee_code,
            format_salary_cents(monthly_salary_cents),
            ot_rate_percent,
            format_date(effective_from)
        ),
    )
    .await?;

    redirect_with_flash(
        &session,
        &format!("/admin/employees/{employee_id}/compensation"),
        "success",
        "Compensation saved",
    )
    .await
}
