use sqlx::{PgPool, Postgres, Transaction};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{CompanySettings, PayrollLineWithEmployee, PayrollRun, PayrollRunStatus};
use crate::services::compensation::get_compensation_as_of;
use crate::services::payroll_controls::is_period_exactly_closed;
use crate::services::reports::{assert_canonical_pay_period, payroll_summary, PayrollFilters};

use super::compute::{
    base_pay_cents_for_period, gross_pay_cents, no_show_deduction_cents, ot_pay_cents,
    GrossPayInput,
};
use super::deductions::refresh_all_line_net_pay;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PayrollRunListItem {
    pub id: Uuid,
    pub period_start: Date,
    pub period_end: Date,
    pub status: PayrollRunStatus,
    pub created_at: OffsetDateTime,
    pub finalized_at: Option<OffsetDateTime>,
    pub line_count: i64,
    pub total_gross_cents: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClosedPeriodCandidate {
    pub period_start: Date,
    pub period_end: Date,
    pub note: Option<String>,
}

pub async fn list_runs(pool: &PgPool) -> AppResult<Vec<PayrollRunListItem>> {
    sqlx::query_as::<_, PayrollRunListItem>(
        "SELECT r.id, r.period_start, r.period_end, r.status, r.created_at, r.finalized_at,
                COUNT(l.id) AS line_count,
                COALESCE(SUM(l.gross_pay_cents), 0) AS total_gross_cents
         FROM payroll_runs r
         LEFT JOIN payroll_lines l ON l.run_id = r.id
         WHERE r.status != 'voided'
         GROUP BY r.id
         ORDER BY r.created_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn list_runnable_closed_periods(pool: &PgPool) -> AppResult<Vec<ClosedPeriodCandidate>> {
    sqlx::query_as::<_, ClosedPeriodCandidate>(
        "SELECT cp.period_start, cp.period_end, cp.note
         FROM closed_pay_periods cp
         WHERE NOT EXISTS (
             SELECT 1 FROM payroll_runs pr
             WHERE pr.period_start = cp.period_start
               AND pr.period_end = cp.period_end
               AND pr.status IN ('draft', 'finalized')
         )
         ORDER BY cp.period_start DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn employees_missing_compensation(pool: &PgPool) -> AppResult<Vec<String>> {
    sqlx::query_scalar(
        "SELECT e.employee_code
         FROM employees e
         LEFT JOIN compensation_profiles c ON c.employee_id = e.id
         WHERE e.is_active = TRUE AND c.employee_id IS NULL
         ORDER BY e.employee_code",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn get_run(pool: &PgPool, run_id: Uuid) -> AppResult<PayrollRun> {
    sqlx::query_as::<_, PayrollRun>(
        "SELECT id, period_start, period_end, status, note, created_by, created_at,
                finalized_at, finalized_by
         FROM payroll_runs WHERE id = $1",
    )
    .bind(run_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::NotFound)
}

pub async fn list_lines_for_run(
    pool: &PgPool,
    run_id: Uuid,
) -> AppResult<Vec<PayrollLineWithEmployee>> {
    sqlx::query_as::<_, PayrollLineWithEmployee>(
        "SELECT l.id, l.employee_id, e.employee_code, e.full_name, p.department,
                e.is_active AS employee_is_active,
                l.regular_minutes, l.approved_ot_minutes, l.pending_ot_minutes, l.no_show_days,
                l.base_pay_cents, l.no_show_deduction_cents, l.ot_pay_cents,
                l.gross_pay_cents, l.net_pay_cents,
                COALESCE((
                    SELECT SUM(d.amount_cents) FROM payroll_deductions d WHERE d.line_id = l.id
                ), 0) AS total_deduction_cents
         FROM payroll_lines l
         JOIN employees e ON e.id = l.employee_id
         LEFT JOIN employee_profiles p ON p.employee_id = e.id
         WHERE l.run_id = $1
         ORDER BY e.full_name",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn create_draft_run(
    pool: &PgPool,
    period_start: Date,
    period_end: Date,
    created_by: Uuid,
    settings: &CompanySettings,
    note: Option<&str>,
) -> AppResult<Uuid> {
    if period_end < period_start {
        return Err(AppError::bad_request(
            "Period end must be on or after period start",
        ));
    }
    if !is_period_exactly_closed(pool, period_start, period_end).await? {
        return Err(AppError::bad_request(
            "Payroll runs require an exactly closed pay period — close this range in Reports first",
        ));
    }
    assert_canonical_pay_period(settings, period_start, period_end)?;

    let missing = employees_missing_compensation(pool).await?;
    if !missing.is_empty() {
        return Err(AppError::bad_request(format!(
            "Set compensation for all active employees before running payroll. Missing: {}",
            missing.join(", ")
        )));
    }

    let summary_rows =
        payroll_summary(pool, period_start, period_end, &PayrollFilters::default()).await?;

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let run_id: Uuid = sqlx::query_scalar(
        "INSERT INTO payroll_runs (period_start, period_end, note, created_by)
         VALUES ($1, $2, $3, $4)
         RETURNING id",
    )
    .bind(period_start)
    .bind(period_end)
    .bind(note.map(str::trim).filter(|n| !n.is_empty()))
    .bind(created_by)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_payroll_run_conflict)?;

    for row in &summary_rows {
        insert_line_for_summary(&mut tx, pool, run_id, row, period_end, settings).await?;
    }

    tx.commit()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(run_id)
}

async fn insert_line_for_summary(
    tx: &mut Transaction<'_, Postgres>,
    pool: &PgPool,
    run_id: Uuid,
    row: &crate::services::reports::PayrollRow,
    period_end: Date,
    settings: &CompanySettings,
) -> AppResult<()> {
    let employee_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM employees WHERE employee_code = $1 AND is_active = TRUE",
    )
    .bind(&row.employee_code)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or_else(|| {
        AppError::bad_request(format!("Active employee not found: {}", row.employee_code))
    })?;

    let comp = get_compensation_as_of(pool, employee_id, period_end)
        .await?
        .ok_or_else(|| {
            AppError::bad_request(format!(
                "Missing compensation effective on {} for {}",
                crate::services::timezone::format_date(period_end),
                row.employee_code
            ))
        })?;

    let base = base_pay_cents_for_period(comp.monthly_salary_cents, settings.pay_period);
    let no_show_ded = no_show_deduction_cents(comp.monthly_salary_cents, row.no_show_days);
    let ot = ot_pay_cents(
        comp.monthly_salary_cents,
        row.approved_ot_minutes,
        comp.ot_rate_percent,
    );
    let gross = gross_pay_cents(&GrossPayInput {
        monthly_salary_cents: comp.monthly_salary_cents,
        ot_rate_percent: comp.ot_rate_percent,
        pay_period: settings.pay_period,
        approved_ot_minutes: row.approved_ot_minutes,
        no_show_days: row.no_show_days,
    });

    sqlx::query(
        "INSERT INTO payroll_lines
            (run_id, employee_id, regular_minutes, approved_ot_minutes, pending_ot_minutes,
             no_show_days, base_pay_cents, no_show_deduction_cents, ot_pay_cents,
             gross_pay_cents, net_pay_cents)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(run_id)
    .bind(employee_id)
    .bind(row.regular_minutes as i32)
    .bind(row.approved_ot_minutes as i32)
    .bind(row.pending_ot_minutes as i32)
    .bind(row.no_show_days as i32)
    .bind(base)
    .bind(no_show_ded)
    .bind(ot)
    .bind(gross)
    .bind(gross)
    .execute(&mut **tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(())
}

fn map_payroll_run_conflict(error: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(db_err) = &error {
        if db_err.constraint() == Some("payroll_runs_one_active_per_period") {
            return AppError::bad_request(
                "A draft or finalized payroll run already exists for this period",
            );
        }
    }
    AppError::Internal(error.into())
}

pub async fn void_draft_run(pool: &PgPool, run_id: Uuid) -> AppResult<()> {
    let run = get_run(pool, run_id).await?;
    if run.status != PayrollRunStatus::Draft {
        return Err(AppError::bad_request(
            "Only draft payroll runs can be voided",
        ));
    }

    let updated =
        sqlx::query("UPDATE payroll_runs SET status = 'voided' WHERE id = $1 AND status = 'draft'")
            .bind(run_id)
            .execute(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn finalize_run(pool: &PgPool, run_id: Uuid, finalized_by: Uuid) -> AppResult<()> {
    let run = get_run(pool, run_id).await?;
    if run.status != PayrollRunStatus::Draft {
        return Err(AppError::bad_request(
            "Only draft payroll runs can be finalized",
        ));
    }

    let line_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM payroll_lines WHERE run_id = $1")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    if line_count == 0 {
        return Err(AppError::bad_request("Payroll run has no employee lines"));
    }

    let pending_ot: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(pending_ot_minutes), 0) FROM payroll_lines WHERE run_id = $1",
    )
    .bind(run_id)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    if pending_ot > 0 {
        return Err(AppError::bad_request(format!(
            "Cannot finalize while {pending_ot} pending OT minutes remain — approve or reject OT first"
        )));
    }

    refresh_all_line_net_pay(pool, run_id).await?;

    let updated = sqlx::query(
        "UPDATE payroll_runs
         SET status = 'finalized', finalized_at = now(), finalized_by = $2
         WHERE id = $1 AND status = 'draft'",
    )
    .bind(run_id)
    .bind(finalized_by)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub fn total_gross_cents(lines: &[PayrollLineWithEmployee]) -> i64 {
    lines.iter().map(|l| l.gross_pay_cents).sum()
}

pub fn total_net_cents(lines: &[PayrollLineWithEmployee]) -> i64 {
    lines.iter().map(|l| l.net_pay_cents).sum()
}

pub fn total_deduction_cents(lines: &[PayrollLineWithEmployee]) -> i64 {
    lines.iter().map(|l| l.total_deduction_cents).sum()
}

pub fn total_pending_ot_minutes(lines: &[PayrollLineWithEmployee]) -> i32 {
    lines.iter().map(|l| l.pending_ot_minutes).sum()
}

pub fn inactive_employee_count(lines: &[PayrollLineWithEmployee]) -> usize {
    lines.iter().filter(|l| !l.employee_is_active).count()
}
