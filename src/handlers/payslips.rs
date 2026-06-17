use axum::extract::{Path, State};
use minijinja::context;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::AppResult;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    compensation::format_salary_cents,
    hours::format_minutes,
    payroll::payslips::{
        get_payslip_for_admin, get_payslip_for_employee, list_payslips_for_employee, PayslipDetail,
    },
    reports::period_label_for_range,
    settings::get_settings,
    timezone::{format_date, format_time},
};
use crate::state::AppState;

fn payslip_template_context(
    payslip: &PayslipDetail,
    company_name: &str,
    timezone: &str,
    back_url: &str,
    back_label: &str,
) -> minijinja::Value {
    let deduction_rows: Vec<_> = payslip
        .deductions
        .iter()
        .map(|d| {
            context! {
                name => d.name.clone(),
                amount => format_salary_cents(d.amount_cents),
                note => d.note.clone().unwrap_or_default(),
            }
        })
        .collect();

    context! {
        line_id => payslip.line_id,
        run_id => payslip.run_id,
        company_name => company_name,
        employee_code => payslip.employee_code.clone(),
        full_name => payslip.full_name.clone(),
        department => payslip.department.clone().unwrap_or_default(),
        period_label => period_label_for_range(payslip.period_start, payslip.period_end),
        period_start => format_date(payslip.period_start),
        period_end => format_date(payslip.period_end),
        regular_minutes => format_minutes(payslip.regular_minutes),
        approved_ot_minutes => format_minutes(payslip.approved_ot_minutes),
        no_show_days => payslip.no_show_days,
        base_pay => format_salary_cents(payslip.base_pay_cents),
        has_no_show_deduction => payslip.no_show_deduction_cents > 0,
        no_show_deduction => format_salary_cents(payslip.no_show_deduction_cents),
        ot_pay => format_salary_cents(payslip.ot_pay_cents),
        has_ot_pay => payslip.ot_pay_cents > 0,
        gross_pay => format_salary_cents(payslip.gross_pay_cents),
        total_deductions => format_salary_cents(payslip.total_deduction_cents),
        has_deductions => payslip.total_deduction_cents > 0,
        net_pay => format_salary_cents(payslip.net_pay_cents),
        deductions => deduction_rows,
        finalized_at => format!(
            "{} {}",
            format_date(payslip.finalized_at.date()),
            format_time(payslip.finalized_at, timezone)
        ),
        back_url => back_url,
        back_label => back_label,
    }
}

pub async fn render_payslip_page(
    state: &AppState,
    session: &Session,
    user: crate::auth::UserSession,
    payslip: PayslipDetail,
    back_url: &str,
    back_label: &str,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    render_page(
        state,
        session,
        Some(user),
        &settings.company_name,
        "Payslip",
        "payslip.html",
        payslip_template_context(
            &payslip,
            &settings.company_name,
            settings.timezone.as_str(),
            back_url,
            back_label,
        ),
    )
    .await
}

pub async fn my_payslips_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let payslips = list_payslips_for_employee(&state.pool, user.employee_id).await?;

    let rows: Vec<_> = payslips
        .iter()
        .map(|p| {
            context! {
                line_id => p.line_id,
                period_label => period_label_for_range(p.period_start, p.period_end),
                gross_pay => format_salary_cents(p.gross_pay_cents),
                deductions => format_salary_cents(p.total_deduction_cents),
                net_pay => format_salary_cents(p.net_pay_cents),
                finalized_at => format!(
                    "{} {}",
                    format_date(p.finalized_at.date()),
                    format_time(p.finalized_at, settings.timezone.as_str())
                ),
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "My Payslips",
        "employee/payslips.html",
        context! {
            payslips => rows,
            has_payslips => !rows.is_empty(),
        },
    )
    .await
}

pub async fn view_my_payslip(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(line_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let payslip = get_payslip_for_employee(&state.pool, user.employee_id, line_id).await?;
    render_payslip_page(
        &state,
        &session,
        user,
        payslip,
        "/me/payslips",
        "My payslips",
    )
    .await
}

pub async fn admin_payslip_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path((run_id, line_id)): Path<(Uuid, Uuid)>,
) -> AppResult<HtmlPage> {
    let payslip = get_payslip_for_admin(&state.pool, run_id, line_id).await?;
    render_payslip_page(
        &state,
        &session,
        user,
        payslip,
        &format!("/admin/payroll/{run_id}"),
        "Payroll run",
    )
    .await
}
