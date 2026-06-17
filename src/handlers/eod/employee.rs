use axum::{
    extract::{Path, State},
    response::Redirect,
    Form,
};
use minijinja::context;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, HtmlPage};
use crate::models::EodReportStatus;
use crate::services::{
    eod::{
        get_report_with_tasks, list_department_eod, list_department_eod_recent,
        list_employee_eod_history, list_tasks, save_report, tasks_to_textareas,
    },
    profile::get_department,
    settings::get_settings,
    timezone::{company_date_now, format_date, format_time},
};
use crate::state::AppState;

use super::common::{collect_tasks, EodForm};

pub async fn my_eod(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let clocked_in =
        crate::services::eod::clocked_in_on_date(&state.pool, user.employee_id, today).await?;

    let (report, tasks) = get_report_with_tasks(&state.pool, user.employee_id, today).await?;
    let is_submitted = report
        .as_ref()
        .is_some_and(|r| r.status == EodReportStatus::Submitted);
    let can_edit = clocked_in && !is_submitted;
    let (completed, pending, blocked, planned) = tasks_to_textareas(&tasks);

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

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "EOD Update",
        "employee/eod.html",
        context! {
            today => format_date(today),
            can_edit => can_edit,
            is_submitted => is_submitted,
            summary => report.as_ref().map(|r| r.summary.clone()).unwrap_or_default(),
            completed => completed,
            pending => pending,
            blocked => blocked,
            planned => planned,
            tasks => task_rows,
            status => report.as_ref().map(|r| match r.status {
                EodReportStatus::Draft => "Draft",
                EodReportStatus::Submitted => "Submitted",
            }).unwrap_or("Not started"),
            submitted => report.as_ref().and_then(|r| r.submitted_at.map(|dt| format_time(dt, &settings.timezone))).unwrap_or_default(),
        },
    )
    .await
}

pub async fn save_my_eod(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<EodForm>,
) -> AppResult<Redirect> {
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let submit = form.is_submit();
    let tasks = collect_tasks(&form);

    if submit && tasks.is_empty() && form.summary_text().trim().is_empty() {
        return Err(AppError::bad_request(
            "Add at least one task or a summary before submitting",
        ));
    }

    save_report(
        &state.pool,
        user.employee_id,
        today,
        form.summary_text(),
        submit,
        &tasks,
    )
    .await?;

    let message = if submit {
        "EOD submitted"
    } else {
        "EOD draft saved"
    };
    redirect_with_flash(&session, "/me/eod", "success", message).await
}

pub async fn team_eod_feed(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let department = get_department(&state.pool, user.employee_id).await?;

    let reports = if let Some(ref dept) = department {
        list_department_eod(&state.pool, user.employee_id, dept, today).await?
    } else {
        Vec::new()
    };

    let recent = if let Some(ref dept) = department {
        let since = today - time::Duration::days(7);
        list_department_eod_recent(&state.pool, dept, since).await?
    } else {
        Vec::new()
    };

    let tz = settings.timezone.as_str();
    let mut today_rows = Vec::new();
    for report in &reports {
        today_rows.push(eod_summary_row(report, &state.pool, tz).await?);
    }

    let recent_rows: Vec<_> = recent
        .iter()
        .filter(|r| r.employee_id != user.employee_id)
        .map(|r| {
            context! {
                employee_code => r.employee_code.clone(),
                full_name => r.full_name.clone(),
                report_date => format_date(r.report_date),
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
        "Team EOD",
        "employee/team_eod.html",
        context! {
            department => department.clone().unwrap_or_default(),
            has_department => department.is_some(),
            today => format_date(today),
            reports => today_rows,
            recent => recent_rows,
        },
    )
    .await
}

async fn eod_summary_row(
    report: &crate::models::EodReportSummary,
    pool: &sqlx::PgPool,
    tz: &str,
) -> AppResult<minijinja::value::Value> {
    let tasks = list_tasks(pool, report.id).await?;
    let task_lines: Vec<_> = tasks
        .iter()
        .map(|t| {
            let kind = match t.kind {
                crate::models::EodTaskKind::Completed => "Done",
                crate::models::EodTaskKind::Pending => "Pending",
                crate::models::EodTaskKind::Blocked => "Blocked",
                crate::models::EodTaskKind::Planned => "Planned",
            };
            context! {
                kind => kind,
                title => t.title.clone(),
            }
        })
        .collect();

    Ok(context! {
        employee_code => report.employee_code.clone(),
        full_name => report.full_name.clone(),
        summary => report.summary.clone(),
        submitted_at => report.submitted_at.map(|dt| format_time(dt, tz)).unwrap_or_default(),
        tasks => task_lines,
    })
}

pub async fn view_eod_detail(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(report_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let department = get_department(&state.pool, user.employee_id).await?;

    let report = sqlx::query_as::<_, crate::models::EodReportSummary>(
        "SELECT er.id, er.employee_id, e.employee_code, e.full_name, p.department,
                er.report_date, er.summary, er.status, er.submitted_at
         FROM eod_reports er
         JOIN employees e ON e.id = er.employee_id
         JOIN employee_profiles p ON p.employee_id = e.id
         WHERE er.id = $1 AND er.status = 'submitted'",
    )
    .bind(report_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::NotFound)?;

    if report.employee_id != user.employee_id {
        let viewer_dept = department.as_deref();
        let report_dept = report.department.as_deref();
        if viewer_dept.is_none() || report_dept.is_none() || viewer_dept != report_dept {
            return Err(AppError::Forbidden);
        }
    }

    let tasks = list_tasks(&state.pool, report.id).await?;
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

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "EOD Detail",
        "employee/eod_detail.html",
        context! {
            employee_code => report.employee_code,
            full_name => report.full_name,
            report_date => format_date(report.report_date),
            summary => report.summary,
            submitted_at => report.submitted_at.map(|dt| format_time(dt, &settings.timezone)).unwrap_or_default(),
            tasks => task_rows,
        },
    )
    .await
}

pub async fn my_eod_history(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let history = list_employee_eod_history(&state.pool, user.employee_id, 60).await?;

    let tz = settings.timezone.as_str();
    let rows: Vec<_> = history
        .iter()
        .map(|item| {
            context! {
                id => item.id,
                report_date => format_date(item.report_date),
                summary => item.summary.clone(),
                submitted_at => item.submitted_at.map(|dt| format_time(dt, tz)).unwrap_or_default(),
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "EOD History",
        "employee/eod_history.html",
        context! { reports => rows },
    )
    .await
}
