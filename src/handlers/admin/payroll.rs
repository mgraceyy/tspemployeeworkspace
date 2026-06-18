use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
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
        inactive_employee_count, list_lines_for_run, list_runnable_closed_periods, list_runs,
        total_deduction_cents, total_gross_cents, total_net_cents, total_pending_ot_minutes,
        void_draft_run,
    },
    payroll::{
        build_finalized_run_csv, get_line_for_run, list_deduction_types, list_deductions_for_line,
        parse_optional_amount_to_cents, save_line_deductions, DeductionInput,
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

fn build_deduction_inputs_from_form(
    types: &[crate::models::DeductionType],
    form: &HashMap<String, String>,
) -> AppResult<Vec<DeductionInput>> {
    let mut inputs = Vec::new();
    for dtype in types {
        let amount_key = format!("amount_{}", dtype.code.to_lowercase());
        let note_key = format!("note_{}", dtype.code.to_lowercase());
        let amount_cents = parse_optional_amount_to_cents(
            form.get(&amount_key).map(|s| s.as_str()).unwrap_or(""),
        )?;
        let note = form
            .get(&note_key)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        inputs.push(DeductionInput {
            deduction_type_id: dtype.id,
            amount_cents,
            note,
        });
    }
    Ok(inputs)
}

pub async fn payroll_runs_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let runs = list_runs(&state.pool).await?;
    let candidates = list_runnable_closed_periods(&state.pool).await?;
    let mut missing_comp = Vec::new();
    for candidate in &candidates {
        for code in employees_missing_compensation(&state.pool, candidate.period_end).await? {
            if !missing_comp.contains(&code) {
                missing_comp.push(code);
            }
        }
    }
    missing_comp.sort();

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
    state.metrics.record_payroll_run_created();

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
    if run.status == PayrollRunStatus::Voided {
        return Err(AppError::NotFound);
    }
    let lines = list_lines_for_run(&state.pool, run_id).await?;
    let pending_ot = total_pending_ot_minutes(&lines);
    let inactive_count = inactive_employee_count(&lines);
    let total_gross = total_gross_cents(&lines);
    let total_deductions = total_deduction_cents(&lines);
    let total_net = total_net_cents(&lines);

    let line_rows: Vec<_> = lines
        .iter()
        .map(|l| {
            context! {
                line_id => l.id,
                employee_code => l.employee_code.clone(),
                full_name => l.full_name.clone(),
                department => l.department.clone().unwrap_or_default(),
                is_inactive => !l.employee_is_active,
                no_show_days => l.no_show_days,
                approved_ot_minutes => l.approved_ot_minutes,
                pending_ot_minutes => l.pending_ot_minutes,
                base_pay => format_salary_cents(l.base_pay_cents),
                no_show_deduction => format_salary_cents(l.no_show_deduction_cents),
                ot_pay => format_salary_cents(l.ot_pay_cents),
                gross_pay => format_salary_cents(l.gross_pay_cents),
                deductions => format_salary_cents(l.total_deduction_cents),
                has_deductions => l.total_deduction_cents > 0,
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
            total_deductions => format_salary_cents(total_deductions),
            total_net => format_salary_cents(total_net),
            has_pending_ot => pending_ot > 0,
            pending_ot_minutes => pending_ot,
            can_finalize => pending_ot == 0,
            has_inactive_employees => inactive_count > 0,
            inactive_employee_count => inactive_count,
            is_finalized => run.status == PayrollRunStatus::Finalized,
        },
    )
    .await
}

pub async fn export_payroll_run_csv(
    State(state): State<AppState>,
    AuthUser(_user): AuthUser,
    Path(run_id): Path<Uuid>,
) -> AppResult<Response> {
    let settings = get_settings(&state.pool).await?;
    let run = get_run(&state.pool, run_id).await?;
    if run.status == PayrollRunStatus::Voided {
        return Err(AppError::NotFound);
    }
    let period_label = period_label_for_range(run.period_start, run.period_end);
    let csv_bytes = build_finalized_run_csv(&state.pool, &run, &period_label).await?;
    let filename = format!(
        "{}-payroll-run-{}.csv",
        settings.company_name.replace(' ', "-"),
        period_label.replace(' ', "-")
    );
    let disposition = format!("attachment; filename=\"{filename}\"");
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "text/csv".to_string()),
            (axum::http::header::CONTENT_DISPOSITION, disposition),
        ],
        csv_bytes,
    )
        .into_response())
}

pub async fn payroll_line_deductions_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path((run_id, line_id)): Path<(Uuid, Uuid)>,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let run = get_run(&state.pool, run_id).await?;
    if run.status == PayrollRunStatus::Voided {
        return Err(AppError::NotFound);
    }
    let line = get_line_for_run(&state.pool, run_id, line_id).await?;
    let types = list_deduction_types(&state.pool).await?;
    let existing = list_deductions_for_line(&state.pool, line_id).await?;
    let is_draft = run.status == PayrollRunStatus::Draft;

    let type_rows: Vec<_> = types
        .iter()
        .map(|t| {
            let saved = existing.iter().find(|d| d.deduction_type_id == t.id);
            let (amount, note) = if let Some(d) = saved {
                (
                    format_salary_cents(d.amount_cents),
                    d.note.clone().unwrap_or_default(),
                )
            } else {
                (String::new(), String::new())
            };
            context! {
                code => t.code.clone(),
                code_lower => t.code.to_lowercase(),
                name => t.name.clone(),
                amount => amount,
                note => note,
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Payroll Deductions",
        "admin/payroll_line_deductions.html",
        context! {
            run_id => run_id,
            line_id => line_id,
            period_label => period_label_for_range(run.period_start, run.period_end),
            status => match run.status {
                PayrollRunStatus::Draft => "Draft",
                PayrollRunStatus::Finalized => "Finalized",
                PayrollRunStatus::Voided => "Voided",
            },
            is_draft => is_draft,
            employee_code => line.employee_code,
            full_name => line.full_name,
            is_inactive => !line.employee_is_active,
            gross_pay => format_salary_cents(line.gross_pay_cents),
            total_deductions => format_salary_cents(line.total_deduction_cents),
            net_pay => format_salary_cents(line.net_pay_cents),
            deduction_types => type_rows,
        },
    )
    .await
}

pub async fn save_payroll_line_deductions_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path((run_id, line_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<HashMap<String, String>>,
) -> AppResult<Redirect> {
    let types = list_deduction_types(&state.pool).await?;
    let inputs = build_deduction_inputs_from_form(&types, &form)?;
    save_line_deductions(&state.pool, run_id, line_id, &inputs).await?;

    let line = get_line_for_run(&state.pool, run_id, line_id).await?;
    log_action(
        &state.pool,
        user.employee_id,
        "payroll.deductions_saved",
        &format!(
            "Updated payroll deductions for {} — net PHP {}",
            line.employee_code,
            format_salary_cents(line.net_pay_cents)
        ),
    )
    .await?;

    redirect_with_flash(
        &session,
        &format!("/admin/payroll/{run_id}"),
        "success",
        &format!(
            "Deductions saved for {} — net pay PHP {}",
            line.employee_code,
            format_salary_cents(line.net_pay_cents)
        ),
    )
    .await
}

pub async fn void_payroll_run_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(run_id): Path<Uuid>,
) -> AppResult<Redirect> {
    let run = get_run(&state.pool, run_id).await?;
    void_draft_run(&state.pool, run_id).await?;
    state.metrics.record_payroll_run_voided();

    log_action(
        &state.pool,
        user.employee_id,
        "payroll.run_voided",
        &format!(
            "Voided draft payroll run for {} to {}",
            format_date(run.period_start),
            format_date(run.period_end)
        ),
    )
    .await?;

    redirect_with_flash(
        &session,
        "/admin/payroll",
        "success",
        "Draft payroll run voided — you can create a new run for this period",
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
    state.metrics.record_payroll_run_finalized();

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
        "Payroll run finalized — gross pay and deductions are locked for this period",
    )
    .await
}
