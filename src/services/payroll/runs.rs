use sqlx::{PgPool, Postgres, Transaction};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{CompanySettings, PayrollLineWithEmployee, PayrollRun, PayrollRunStatus};
use crate::services::compensation::get_compensation_map_as_of;
use crate::services::payroll_controls::is_period_exactly_closed;
use crate::services::reports::{assert_canonical_pay_period, payroll_summary, PayrollFilters};

use super::compute::{
    allowance_pay_cents_for_period, base_pay_cents_for_period, gross_pay_cents,
    no_show_deduction_cents, ot_pay_cents, GrossPayInput,
};
use super::deductions::{apply_deduction_defaults_for_run, refresh_all_line_net_pay};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

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

pub async fn employees_missing_compensation(pool: &PgPool, as_of: Date) -> AppResult<Vec<String>> {
    sqlx::query_scalar(
        "SELECT e.employee_code
         FROM employees e
         WHERE e.is_active = TRUE
           AND NOT EXISTS (
             SELECT 1 FROM compensation_profiles c
             WHERE c.employee_id = e.id AND c.effective_from <= $1
           )
           AND NOT EXISTS (
             SELECT 1 FROM compensation_history h
             WHERE h.employee_id = e.id
               AND h.effective_from <= $1
               AND (h.effective_to IS NULL OR h.effective_to >= $1)
           )
         ORDER BY e.employee_code",
    )
    .bind(as_of)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PeriodPayrollStatus {
    pub id: Uuid,
    pub status: PayrollRunStatus,
}

pub async fn get_active_run_for_period(
    pool: &PgPool,
    period_start: Date,
    period_end: Date,
) -> AppResult<Option<PeriodPayrollStatus>> {
    sqlx::query_as::<_, PeriodPayrollStatus>(
        "SELECT id, status
         FROM payroll_runs
         WHERE period_start = $1 AND period_end = $2 AND status IN ('draft', 'finalized')
         LIMIT 1",
    )
    .bind(period_start)
    .bind(period_end)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn get_run(pool: &PgPool, run_id: Uuid) -> AppResult<PayrollRun> {
    sqlx::query_as::<_, PayrollRun>(
        "SELECT id, period_start, period_end, status, note, created_by, created_at,
                finalized_at, finalized_by, attendance_snapshot_hash
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
                l.base_pay_cents, l.allowance_cents, l.no_show_deduction_cents, l.ot_pay_cents,
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

    let summary_rows =
        payroll_summary(pool, period_start, period_end, &PayrollFilters::default()).await?;

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let snapshot_hash = attendance_snapshot_hash(&summary_rows);
    let run_id: Uuid = sqlx::query_scalar(
        "INSERT INTO payroll_runs (period_start, period_end, note, created_by, attendance_snapshot_hash)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id",
    )
    .bind(period_start)
    .bind(period_end)
    .bind(note.map(str::trim).filter(|n| !n.is_empty()))
    .bind(created_by)
    .bind(&snapshot_hash)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_payroll_run_conflict)?;

    let employee_codes: Vec<String> = summary_rows
        .iter()
        .map(|row| row.employee_code.clone())
        .collect();
    let employee_map = active_employee_ids_by_code(&mut tx, &employee_codes).await?;
    let employee_ids: Vec<Uuid> = employee_map.values().copied().collect();
    let compensation_map = get_compensation_map_as_of(pool, &employee_ids, period_end).await?;

    for row in &summary_rows {
        insert_line_for_summary(
            &mut tx,
            run_id,
            row,
            period_end,
            settings,
            &employee_map,
            &compensation_map,
        )
        .await?;
    }

    tx.commit()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    apply_deduction_defaults_for_run(pool, run_id).await?;
    Ok(run_id)
}

async fn active_employee_ids_by_code(
    tx: &mut Transaction<'_, Postgres>,
    employee_codes: &[String],
) -> AppResult<std::collections::HashMap<String, Uuid>> {
    if employee_codes.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let rows: Vec<(String, Uuid)> = sqlx::query_as(
        "SELECT employee_code, id FROM employees
         WHERE employee_code = ANY($1) AND is_active = TRUE",
    )
    .bind(employee_codes)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows.into_iter().collect())
}

async fn insert_line_for_summary(
    tx: &mut Transaction<'_, Postgres>,
    run_id: Uuid,
    row: &crate::services::reports::PayrollRow,
    _period_end: Date,
    settings: &CompanySettings,
    employee_map: &std::collections::HashMap<String, Uuid>,
    compensation_map: &std::collections::HashMap<Uuid, crate::models::CompensationProfile>,
) -> AppResult<()> {
    let employee_id = employee_map
        .get(&row.employee_code)
        .copied()
        .ok_or_else(|| {
            AppError::bad_request(format!("Active employee not found: {}", row.employee_code))
        })?;

    let Some(comp) = compensation_map.get(&employee_id) else {
        tracing::warn!(
            employee_code = %row.employee_code,
            "Skipping payroll line: no compensation effective on period end"
        );
        return Ok(());
    };

    let base = base_pay_cents_for_period(comp.monthly_salary_cents, settings.pay_period);
    let allowance =
        allowance_pay_cents_for_period(comp.monthly_allowance_cents(), settings.pay_period);
    let no_show_ded = no_show_deduction_cents(comp.monthly_salary_cents, row.no_show_days);
    let ot = ot_pay_cents(
        comp.monthly_salary_cents,
        row.approved_ot_minutes,
        comp.ot_rate_percent,
    );
    let gross = gross_pay_cents(&GrossPayInput {
        monthly_salary_cents: comp.monthly_salary_cents,
        monthly_allowance_cents: comp.monthly_allowance_cents(),
        ot_rate_percent: comp.ot_rate_percent,
        pay_period: settings.pay_period,
        approved_ot_minutes: row.approved_ot_minutes,
        no_show_days: row.no_show_days,
    });

    sqlx::query(
        "INSERT INTO payroll_lines
            (run_id, employee_id, regular_minutes, approved_ot_minutes, pending_ot_minutes,
             no_show_days, base_pay_cents, allowance_cents, no_show_deduction_cents, ot_pay_cents,
             gross_pay_cents, net_pay_cents)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(run_id)
    .bind(employee_id)
    .bind(row.regular_minutes as i32)
    .bind(row.approved_ot_minutes as i32)
    .bind(row.pending_ot_minutes as i32)
    .bind(row.no_show_days as i32)
    .bind(base)
    .bind(allowance)
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

pub fn attendance_snapshot_hash(rows: &[crate::services::reports::PayrollRow]) -> String {
    let mut sorted: Vec<_> = rows.iter().collect();
    sorted.sort_by(|a, b| a.employee_code.cmp(&b.employee_code));
    let mut hasher = DefaultHasher::new();
    for row in sorted {
        row.employee_code.hash(&mut hasher);
        row.regular_minutes.hash(&mut hasher);
        row.approved_ot_minutes.hash(&mut hasher);
        row.pending_ot_minutes.hash(&mut hasher);
        row.no_show_days.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

pub async fn is_draft_attendance_stale(pool: &PgPool, run: &PayrollRun) -> AppResult<bool> {
    if run.status != PayrollRunStatus::Draft {
        return Ok(false);
    }
    let Some(stored) = run.attendance_snapshot_hash.as_deref() else {
        return Ok(false);
    };
    let current_rows = payroll_summary(
        pool,
        run.period_start,
        run.period_end,
        &PayrollFilters::default(),
    )
    .await?;
    Ok(stored != attendance_snapshot_hash(&current_rows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::reports::PayrollRow;

    fn sample_row(approved_ot_minutes: i64) -> PayrollRow {
        PayrollRow {
            employee_code: "E001".to_string(),
            full_name: "Test Employee".to_string(),
            department: Some("Engineering".to_string()),
            regular_minutes: 9_600,
            approved_ot_minutes,
            pending_ot_minutes: 0,
            sick_leave_days: 0,
            vacation_days: 0,
            official_leave_days: 0,
            offset_days: 0,
            no_show_days: 0,
        }
    }

    #[test]
    fn attendance_snapshot_hash_changes_when_ot_changes() {
        let unchanged = attendance_snapshot_hash(&[sample_row(0)]);
        let changed = attendance_snapshot_hash(&[sample_row(60)]);
        assert_ne!(unchanged, changed);
    }
}
