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
use crate::models::PayrollRunStatus;
use crate::services::{
    audit::log_action,
    compensation::format_salary_cents,
    payroll::runs::{
        create_draft_run, employees_missing_compensation, finalize_run, get_run,
        list_lines_for_run, list_runnable_closed_periods, list_runs, total_gross_cents,
        total_pending_ot_minutes,
    },
    reports::period_label_for_range,
    settings::get_settings,
    timezone::format_date,
};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CreatePayrollRunForm {
    period_start: String,
    period_end: String,
    note: Option<String>,
}

pub async fn payroll_runs_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let runs = list_runs(&state.pool).await?;
    let candidates = list_runnable_closed_periods(&state.pool).await?;
    let missing_comp = employees_missing_compensation(&state.pool).await?;

    let run_rows: Vec<_> = runs
        .iter()
        .map(|r| {
            context! {
                id => r.id,
                period_label => period_label_for_range(r.period_start, r.period_end),
                status => match r.status {
                    PayrollRunStatus::Draft => "Draft",
                    PayrollRunStatus::Finalized => "Finalized",
                    PayrollRunStatus::Voided => "Voided",
                },
                is_draft => r.status == PayrollRunStatus::Draft,
                line_count => r.line_count,
                total_gross => format_salary_cents(r.total_gross_cents),
            }
        })
        .collect();

    let candidate_rows: Vec<_> = candidates
        .iter()
        .map(|c| {
            context! {
                start => format_date(c.period_start),
                end => format_date(c.period_end),
                label => period_label_for_range(c.period_start, c.period_end),
                note => c.note.clone().unwrap_or_default(),
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Payroll Runs",
        "admin/payroll.html",
        context! {
            runs => run_rows,
            candidates => candidate_rows,
            missing_compensation => missing_comp,
            can_create => !candidates.is_empty() && missing_comp.is_empty(),
        },
    )
    .await
}

pub async fn create_payroll_run_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<CreatePayrollRunForm>,
) -> AppResult<Redirect> {
    let settings = get_settings(&state.pool).await?;
    let start =
        crate::services::timezone::parse_date(&form.period_start).map_err(AppError::bad_request)?;
    let end =
        crate::services::timezone::parse_date(&form.period_end).map_err(AppError::bad_request)?;

    let run_id = create_draft_run(
        &state.pool,
        start,
        end,
        user.employee_id,
        &settings,
        form.note.as_deref(),
    )
    .await?;

    log_action(
        &state.pool,
        user.employee_id,
        "payroll.run_created",
        &format!(
            "Created draft payroll run for {} to {}",
            format_date(start),
            format_date(end)
        ),
    )
    .await?;

    redirect_with_flash(
        &session,
        &format!("/admin/payroll/{run_id}"),
        "success",
        "Draft payroll run created",
    )
    .await
}

pub async fn payroll_run_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(run_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let run = get_run(&state.pool, run_id).await?;
    let lines = list_lines_for_run(&state.pool, run_id).await?;
    let pending_ot = total_pending_ot_minutes(&lines);
    let total_gross = total_gross_cents(&lines);

    let line_rows: Vec<_> = lines
        .iter()
        .map(|l| {
            context! {
                employee_code => l.employee_code.clone(),
                full_name => l.full_name.clone(),
                department => l.department.clone().unwrap_or_default(),
                no_show_days => l.no_show_days,
                approved_ot_minutes => l.approved_ot_minutes,
                pending_ot_minutes => l.pending_ot_minutes,
                base_pay => format_salary_cents(l.base_pay_cents),
                no_show_deduction => format_salary_cents(l.no_show_deduction_cents),
                ot_pay => format_salary_cents(l.ot_pay_cents),
                gross_pay => format_salary_cents(l.gross_pay_cents),
                net_pay => format_salary_cents(l.net_pay_cents),
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Payroll Run",
        "admin/payroll_run.html",
        context! {
            run_id => run_id,
            period_label => period_label_for_range(run.period_start, run.period_end),
            status => match run.status {
                PayrollRunStatus::Draft => "Draft",
                PayrollRunStatus::Finalized => "Finalized",
                PayrollRunStatus::Voided => "Voided",
            },
            is_draft => run.status == PayrollRunStatus::Draft,
            note => run.note.clone().unwrap_or_default(),
            lines => line_rows,
            line_count => lines.len(),
            total_gross => format_salary_cents(total_gross),
            has_pending_ot => pending_ot > 0,
            pending_ot_minutes => pending_ot,
        },
    )
    .await
}

pub async fn finalize_payroll_run_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(run_id): Path<Uuid>,
) -> AppResult<Redirect> {
    let run = get_run(&state.pool, run_id).await?;
    finalize_run(&state.pool, run_id, user.employee_id).await?;

    log_action(
        &state.pool,
        user.employee_id,
        "payroll.run_finalized",
        &format!(
            "Finalized payroll run for {} to {}",
            format_date(run.period_start),
            format_date(run.period_end)
        ),
    )
    .await?;

    redirect_with_flash(
        &session,
        &format!("/admin/payroll/{run_id}"),
        "success",
        "Payroll run finalized — gross pay is locked for this period",
    )
    .await
}
