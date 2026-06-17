use axum::{
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
};
use minijinja::context;
use serde::Deserialize;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::display::entry_row;
use crate::error::AppResult;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    clock::list_entries_for_employee_range,
    reports::{build_timesheet_csv, resolve_timesheet_period},
    settings::get_settings,
    team::{assert_can_manage, get_employee_summary},
    timezone::{company_date_now, format_date},
};
use crate::state::AppState;

#[derive(Deserialize, Default)]
pub struct TimesheetQuery {
    start: Option<String>,
    end: Option<String>,
}

pub async fn team_timesheet(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
    Query(query): Query<TimesheetQuery>,
) -> AppResult<HtmlPage> {
    let is_admin = user.role.is_admin();
    assert_can_manage(&state.pool, user.employee_id, employee_id, is_admin).await?;

    let settings = get_settings(&state.pool).await?;
    let employee = get_employee_summary(&state.pool, employee_id).await?;
    let today = company_date_now(&settings)?;
    let (start, end) =
        resolve_timesheet_period(today, query.start.as_deref(), query.end.as_deref())?;
    let entries = list_entries_for_employee_range(&state.pool, employee_id, start, end).await?;
    let tz = settings.timezone.as_str();
    let rows: Vec<_> = entries.iter().map(|e| entry_row(e, tz)).collect();
    let export_query = if query.start.is_some() && query.end.is_some() {
        format!("?start={}&end={}", format_date(start), format_date(end))
    } else {
        String::new()
    };
    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Team Timesheet",
        "manager/team_timesheet.html",
        context! {
            employee => employee,
            entries => rows,
            start_date => format_date(start),
            end_date => format_date(end),
            export_query => export_query,
        },
    )
    .await
}

pub async fn export_team_timesheet_csv(
    State(state): State<AppState>,
    _session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
    Query(query): Query<TimesheetQuery>,
) -> AppResult<Response> {
    let is_admin = user.role.is_admin();
    assert_can_manage(&state.pool, user.employee_id, employee_id, is_admin).await?;

    let settings = get_settings(&state.pool).await?;
    let employee = get_employee_summary(&state.pool, employee_id).await?;
    let today = company_date_now(&settings)?;
    let (start, end) =
        resolve_timesheet_period(today, query.start.as_deref(), query.end.as_deref())?;
    let entries = list_entries_for_employee_range(&state.pool, employee_id, start, end).await?;
    let csv_bytes = build_timesheet_csv(
        &employee.employee_code,
        &employee.full_name,
        start,
        end,
        &entries,
        &settings.timezone,
    )?;

    let filename = format!(
        "{}-timesheet-{}-{}.csv",
        employee.employee_code,
        format_date(start),
        format_date(end)
    );
    let disposition = format!("attachment; filename=\"{filename}\"");

    Ok((
        [
            (header::CONTENT_TYPE, "text/csv".to_string()),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        csv_bytes,
    )
        .into_response())
}
