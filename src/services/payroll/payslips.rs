use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use super::deductions::list_deductions_for_line;
use crate::error::{AppError, AppResult};
use crate::models::{PayrollDeductionWithType, PayrollRunStatus};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PayslipListItem {
    pub line_id: Uuid,
    pub run_id: Uuid,
    pub period_start: Date,
    pub period_end: Date,
    pub gross_pay_cents: i64,
    pub net_pay_cents: i64,
    pub total_deduction_cents: i64,
    pub finalized_at: OffsetDateTime,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PayslipLineRow {
    pub line_id: Uuid,
    pub run_id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub department: Option<String>,
    pub period_start: Date,
    pub period_end: Date,
    pub run_status: PayrollRunStatus,
    pub regular_minutes: i32,
    pub approved_ot_minutes: i32,
    pub no_show_days: i32,
    pub base_pay_cents: i64,
    pub allowance_cents: i64,
    pub no_show_deduction_cents: i64,
    pub ot_pay_cents: i64,
    pub gross_pay_cents: i64,
    pub net_pay_cents: i64,
    pub total_deduction_cents: i64,
    pub finalized_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone)]
pub struct PayslipDetail {
    pub line_id: Uuid,
    pub run_id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub department: Option<String>,
    pub period_start: Date,
    pub period_end: Date,
    pub regular_minutes: i32,
    pub approved_ot_minutes: i32,
    pub no_show_days: i32,
    pub base_pay_cents: i64,
    pub allowance_cents: i64,
    pub no_show_deduction_cents: i64,
    pub ot_pay_cents: i64,
    pub gross_pay_cents: i64,
    pub total_deduction_cents: i64,
    pub net_pay_cents: i64,
    pub deductions: Vec<PayrollDeductionWithType>,
    pub finalized_at: OffsetDateTime,
}

pub async fn list_payslips_for_employee(
    pool: &PgPool,
    employee_id: Uuid,
) -> AppResult<Vec<PayslipListItem>> {
    sqlx::query_as::<_, PayslipListItem>(
        "SELECT l.id AS line_id, r.id AS run_id, r.period_start, r.period_end,
                l.gross_pay_cents, l.net_pay_cents,
                COALESCE((
                    SELECT SUM(d.amount_cents) FROM payroll_deductions d WHERE d.line_id = l.id
                ), 0) AS total_deduction_cents,
                r.finalized_at
         FROM payroll_lines l
         JOIN payroll_runs r ON r.id = l.run_id
         WHERE l.employee_id = $1 AND r.status = 'finalized'
         ORDER BY r.finalized_at DESC",
    )
    .bind(employee_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

async fn fetch_payslip_line(pool: &PgPool, line_id: Uuid) -> AppResult<PayslipLineRow> {
    sqlx::query_as::<_, PayslipLineRow>(
        "SELECT l.id AS line_id, r.id AS run_id, l.employee_id, e.employee_code, e.full_name,
                p.department, r.period_start, r.period_end, r.status AS run_status,
                l.regular_minutes, l.approved_ot_minutes, l.no_show_days,
                l.base_pay_cents, l.allowance_cents, l.no_show_deduction_cents, l.ot_pay_cents,
                l.gross_pay_cents, l.net_pay_cents,
                COALESCE((
                    SELECT SUM(d.amount_cents) FROM payroll_deductions d WHERE d.line_id = l.id
                ), 0) AS total_deduction_cents,
                r.finalized_at
         FROM payroll_lines l
         JOIN payroll_runs r ON r.id = l.run_id
         JOIN employees e ON e.id = l.employee_id
         LEFT JOIN employee_profiles p ON p.employee_id = e.id
         WHERE l.id = $1",
    )
    .bind(line_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::NotFound)
}

fn row_to_detail(
    row: PayslipLineRow,
    deductions: Vec<PayrollDeductionWithType>,
) -> AppResult<PayslipDetail> {
    let finalized_at = row.finalized_at.ok_or_else(|| {
        AppError::bad_request("Payslips are only available for finalized payroll runs")
    })?;

    Ok(PayslipDetail {
        line_id: row.line_id,
        run_id: row.run_id,
        employee_id: row.employee_id,
        employee_code: row.employee_code,
        full_name: row.full_name,
        department: row.department,
        period_start: row.period_start,
        period_end: row.period_end,
        regular_minutes: row.regular_minutes,
        approved_ot_minutes: row.approved_ot_minutes,
        no_show_days: row.no_show_days,
        base_pay_cents: row.base_pay_cents,
        allowance_cents: row.allowance_cents,
        no_show_deduction_cents: row.no_show_deduction_cents,
        ot_pay_cents: row.ot_pay_cents,
        gross_pay_cents: row.gross_pay_cents,
        total_deduction_cents: row.total_deduction_cents,
        net_pay_cents: row.net_pay_cents,
        deductions,
        finalized_at,
    })
}

pub async fn get_payslip_for_employee(
    pool: &PgPool,
    employee_id: Uuid,
    line_id: Uuid,
) -> AppResult<PayslipDetail> {
    let row = fetch_payslip_line(pool, line_id).await?;
    if row.employee_id != employee_id {
        return Err(AppError::NotFound);
    }
    if row.run_status != PayrollRunStatus::Finalized {
        return Err(AppError::NotFound);
    }
    let deductions = list_deductions_for_line(pool, line_id).await?;
    row_to_detail(row, deductions)
}

pub async fn get_payslip_for_admin(
    pool: &PgPool,
    run_id: Uuid,
    line_id: Uuid,
) -> AppResult<PayslipDetail> {
    let row = fetch_payslip_line(pool, line_id).await?;
    if row.run_id != run_id {
        return Err(AppError::NotFound);
    }
    if row.run_status != PayrollRunStatus::Finalized {
        return Err(AppError::NotFound);
    }
    let deductions = list_deductions_for_line(pool, line_id).await?;
    row_to_detail(row, deductions)
}
