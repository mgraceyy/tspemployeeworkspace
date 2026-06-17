use axum::{
    extract::{Path, State},
    response::Redirect,
};
use minijinja::context;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    audit::log_action,
    employees::find_by_id,
    eod::{list_today_submitted_eod, unlock_report},
    settings::get_settings,
    timezone::{company_date_now, format_date, format_time},
};
use crate::state::AppState;

pub async fn admin_eod_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let reports = list_today_submitted_eod(&state.pool).await?;

    let tz = settings.timezone.as_str();
    let rows: Vec<_> = reports
        .iter()
        .map(|r| {
            context! {
                id => r.id,
                employee_code => r.employee_code.clone(),
                full_name => r.full_name.clone(),
                department => r.department.clone().unwrap_or_default(),
                summary => r.summary.clone(),
                submitted_at => r.submitted_at.map(|dt| format_time(dt, tz)).unwrap_or_default(),
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Unlock EOD",
        "admin/eod.html",
        context! {
            today => format_date(today),
            reports => rows,
        },
    )
    .await
}

pub async fn admin_unlock_eod(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(report_id): Path<Uuid>,
) -> AppResult<Redirect> {
    let report = unlock_report(&state.pool, report_id, user.employee_id).await?;
    let employee = find_by_id(&state.pool, report.employee_id)
        .await?
        .ok_or(AppError::NotFound)?;

    log_action(
        &state.pool,
        user.employee_id,
        "eod.unlocked",
        &format!(
            "Unlocked EOD for {} ({}) on {}",
            employee.full_name,
            employee.employee_code,
            format_date(report.report_date)
        ),
    )
    .await?;

    redirect_with_flash(
        &session,
        "/admin/eod",
        "success",
        &format!("EOD unlocked for {}", employee.full_name),
    )
    .await
}
