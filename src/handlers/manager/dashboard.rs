use axum::extract::State;
use minijinja::context;
use tower_sessions::Session;

use crate::auth::AuthUser;
use crate::display::{ot_pending_row, team_status_row};
use crate::error::AppResult;
use crate::handlers::render::{render_page, HtmlPage};
use crate::models::EmployeeSummary;
use crate::services::{
    employees::list_team,
    eod::count_missing_team_eod,
    ot::{count_pending, list_pending_for_manager},
    settings::get_settings,
    team::{list_manageable_employees, list_team_attendance_today},
    timezone::{company_date_now, format_date},
};
use crate::state::AppState;

pub async fn dashboard(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let is_admin = user.role.is_admin();

    let pending_ot = list_pending_for_manager(&state.pool, user.employee_id, is_admin).await?;
    let team_today = list_team_attendance_today(
        &state.pool,
        user.employee_id,
        is_admin,
        settings.grace_minutes,
    )
    .await?;
    let team: Vec<EmployeeSummary> = if is_admin {
        list_manageable_employees(&state.pool, user.employee_id, true).await?
    } else {
        list_team(&state.pool, user.employee_id).await?
    };

    let tz = settings.timezone.as_str();
    let pending_rows: Vec<_> = pending_ot.iter().map(|e| ot_pending_row(e, tz)).collect();
    let attendance_rows: Vec<_> = team_today.iter().map(|m| team_status_row(m, tz)).collect();
    let pending_count = count_pending(&state.pool, user.employee_id, is_admin).await?;
    let missing_eod_count = count_missing_team_eod(&state.pool, user.employee_id, is_admin).await?;

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Manager Dashboard",
        "manager/dashboard.html",
        context! {
            pending_ot => pending_rows,
            pending_count => pending_count,
            missing_eod_count => missing_eod_count,
            attendance => attendance_rows,
            team => team,
            today => format_date(company_date_now(&settings)?),
        },
    )
    .await
}

pub async fn team_list(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let is_admin = user.role.is_admin();
    let team = list_manageable_employees(&state.pool, user.employee_id, is_admin).await?;

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Team",
        "manager/team_list.html",
        context! {
            team => team,
        },
    )
    .await
}
