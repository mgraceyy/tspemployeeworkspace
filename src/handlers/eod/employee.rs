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
use crate::handlers::flash::redirect_with_flash_from_result;
use crate::handlers::render::{render_page, HtmlPage};
use crate::models::{EodReportStatus, UserRole};
use crate::services::{
    eod::{
        can_view_team_eod_report, get_report_with_tasks, list_employee_eod_history, list_tasks,
        list_team_eod_recent_submissions, list_team_eod_submissions, save_report,
        tasks_to_textareas,
    },
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

    let history = list_employee_eod_history(&state.pool, user.employee_id, 60).await?;
    let tz = settings.timezone.as_str();
    let history_rows: Vec<_> = history
        .iter()
        .filter(|item| item.report_date != today)
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
        "EOD",
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
            history => history_rows,
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

    let message = if submit {
        "EOD submitted"
    } else {
        "EOD draft saved"
    };

    let result: AppResult<()> = async {
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
        Ok(())
    }
    .await;

    redirect_with_flash_from_result(&session, "/me/eod", message, result).await
}

pub async fn team_eod_feed(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let is_admin = user.role.is_admin();
    let manages_team = user.role.is_manager_or_admin();

    let reports =
        list_team_eod_submissions(&state.pool, user.employee_id, is_admin, manages_team, today)
            .await?;

    let since = today - time::Duration::days(7);
    let recent = list_team_eod_recent_submissions(
        &state.pool,
        user.employee_id,
        is_admin,
        manages_team,
        since,
    )
    .await?;

    let has_team = manages_team
        || sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT manager_id FROM employees WHERE id = $1 AND is_active = TRUE",
        )
        .bind(user.employee_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .is_some();

    let team_scope = if is_admin {
        "All employees"
    } else if user.role == UserRole::Manager {
        "Your direct reports"
    } else {
        "Your team"
    };

    let tz = settings.timezone.as_str();
    let mut today_rows = Vec::new();
    for report in &reports {
        today_rows.push(eod_summary_row(report, &state.pool, tz).await?);
    }

    let recent_rows: Vec<_> = recent
        .iter()
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
            team_scope => team_scope,
            has_team => has_team,
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
    let is_admin = user.role.is_admin();
    let manages_team = user.role.is_manager_or_admin();

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

    let allowed = can_view_team_eod_report(
        &state.pool,
        user.employee_id,
        report.employee_id,
        is_admin,
        manages_team,
    )
    .await?;
    if !allowed {
        return Err(AppError::Forbidden);
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

pub async fn my_eod_history() -> Redirect {
    Redirect::to("/me/eod")
}
