use axum::{
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
};
use minijinja::context;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::handlers::render::{render_page, HtmlPage};
use crate::models::EodReportStatus;
use crate::services::{
    eod::{
        build_eod_weekly_csv, get_report_with_tasks, list_team_eod_export_rows,
        list_team_eod_status,
    },
    settings::get_settings,
    team::assert_can_manage,
    timezone::{company_date_now, format_date},
};
use crate::state::AppState;

pub async fn manager_eod_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let is_admin = user.role.is_admin();
    let rows = list_team_eod_status(&state.pool, user.employee_id, is_admin, today).await?;

    let team_rows: Vec<_> = rows
        .iter()
        .map(|r| {
            let eod_label = match r.eod_status {
                Some(EodReportStatus::Submitted) => "Submitted",
                Some(EodReportStatus::Draft) => "Draft",
                None if r.clocked_in => "Missing",
                None => "—",
            };
            context! {
                employee_id => r.employee_id,
                employee_code => r.employee_code.clone(),
                full_name => r.full_name.clone(),
                clocked_in => r.clocked_in,
                eod_status => eod_label,
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Team EOD Status",
        "manager/eod.html",
        context! {
            today => format_date(today),
            team => team_rows,
        },
    )
    .await
}

pub async fn manager_export_weekly_csv(
    State(state): State<AppState>,
    _session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<Response> {
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let since = today - time::Duration::days(6);
    let is_admin = user.role.is_admin();
    let rows =
        list_team_eod_export_rows(&state.pool, user.employee_id, is_admin, since, today).await?;
    let csv_bytes = build_eod_weekly_csv(&state.pool, &rows).await?;

    let filename = format!(
        "{}-eod-week-{}.csv",
        settings.company_name.replace(' ', "-"),
        format_date(today)
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

pub async fn manager_view_eod(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let is_admin = user.role.is_admin();
    assert_can_manage(&state.pool, user.employee_id, employee_id, is_admin).await?;

    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let (report, tasks) = get_report_with_tasks(&state.pool, employee_id, today).await?;

    let Some(report) = report else {
        return Err(AppError::NotFound);
    };

    let task_rows: Vec<_> = tasks
        .iter()
        .map(|t| {
            let kind = match t.kind {
                crate::models::EodTaskKind::Completed => "Completed",
                crate::models::EodTaskKind::Pending => "Pending",
                crate::models::EodTaskKind::Blocked => "Blocked",
                crate::models::EodTaskKind::Planned => "Planned",
            };
            context! { kind => kind, title => t.title.clone() }
        })
        .collect();

    let work = crate::services::profile::get_work_profile(&state.pool, employee_id).await?;

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Team Member EOD",
        "manager/eod_detail.html",
        context! {
            employee_code => work.employee_code,
            full_name => work.full_name,
            department => work.department.unwrap_or_default(),
            report_date => format_date(today),
            summary => report.summary,
            status => match report.status {
                EodReportStatus::Draft => "Draft",
                EodReportStatus::Submitted => "Submitted",
            },
            tasks => task_rows,
        },
    )
    .await
}
