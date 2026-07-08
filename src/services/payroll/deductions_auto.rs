//! Government auto-deduction application for payroll runs.
//! Wire into payroll/mod.rs: `mod deductions_auto;` and re-export public items.

use std::collections::HashMap;

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{CompanySettings, PayPeriodType, PayrollRunStatus};
use crate::services::compensation::get_compensation_map_as_of;

use super::compute::lwop_deduction_cents;
use super::deductions::refresh_all_line_net_pay;
use super::gov_deductions::{
    is_government_deduction_code, period_government_deductions_cents, GovernmentDeductionToggles,
    AUTO_NOTE, GOV_CODE_HDMF, GOV_CODE_PHIC, GOV_CODE_SSS, GOV_CODE_WHT, LWOP_CODE,
};
use super::runs::get_run;

/// Applies employee deduction defaults and government auto-deductions for every line in a draft run.
pub async fn apply_automatic_deductions_for_run(
    pool: &PgPool,
    run_id: Uuid,
    settings: &CompanySettings,
) -> AppResult<()> {
    let run = get_run(pool, run_id).await?;
    if run.status != PayrollRunStatus::Draft {
        return Err(AppError::bad_request(
            "Automatic deductions can only be applied to draft payroll runs",
        ));
    }

    let toggles = load_government_toggles(pool).await?;
    let type_map = government_type_ids(pool).await?;

    let lwop_type_id = lwop_type_id(pool).await?;

    let lines: Vec<(Uuid, Uuid, i64, i32)> = sqlx::query_as(
        "SELECT l.id, l.employee_id, l.gross_pay_cents, l.lwop_days FROM payroll_lines l WHERE l.run_id = $1",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let employee_ids: Vec<Uuid> = lines.iter().map(|(_, eid, _, _)| *eid).collect();
    let compensation_map = get_compensation_map_as_of(pool, &employee_ids, run.period_end).await?;

    for (line_id, employee_id, gross_cents, lwop_days) in lines {
        apply_employee_defaults_for_line(pool, line_id, employee_id, gross_cents).await?;
        apply_government_deductions_for_line(
            pool,
            line_id,
            employee_id,
            gross_cents,
            settings.pay_period,
            toggles,
            &type_map,
            compensation_map.get(&employee_id),
        )
        .await?;
        apply_lwop_deduction_for_line(
            pool,
            line_id,
            employee_id,
            gross_cents,
            lwop_days,
            lwop_type_id,
            compensation_map.get(&employee_id),
        )
        .await?;
    }

    refresh_all_line_net_pay(pool, run_id).await
}

/// Recomputes government auto-deductions for all lines (removes stale auto rows first).
pub async fn recalculate_government_deductions_for_run(
    pool: &PgPool,
    run_id: Uuid,
    settings: &CompanySettings,
) -> AppResult<()> {
    let run = get_run(pool, run_id).await?;
    if run.status != PayrollRunStatus::Draft {
        return Err(AppError::bad_request(
            "Government deductions can only be recalculated on draft payroll runs",
        ));
    }

    sqlx::query(
        "DELETE FROM payroll_deductions d
         USING payroll_lines l, deduction_types t
         WHERE d.line_id = l.id
           AND l.run_id = $1
           AND d.deduction_type_id = t.id
           AND t.code = ANY($2)
           AND d.note = $3",
    )
    .bind(run_id)
    .bind(&[GOV_CODE_SSS, GOV_CODE_PHIC, GOV_CODE_HDMF, GOV_CODE_WHT][..])
    .bind(AUTO_NOTE)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let toggles = load_government_toggles(pool).await?;
    if !toggles.any_enabled() {
        return recalculate_lwop_deductions_for_run(pool, run_id).await;
    }

    let type_map = government_type_ids(pool).await?;
    let lines: Vec<(Uuid, Uuid, i64)> = sqlx::query_as(
        "SELECT l.id, l.employee_id, l.gross_pay_cents FROM payroll_lines l WHERE l.run_id = $1",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let employee_ids: Vec<Uuid> = lines.iter().map(|(_, eid, _)| *eid).collect();
    let compensation_map = get_compensation_map_as_of(pool, &employee_ids, run.period_end).await?;

    for (line_id, employee_id, gross_cents) in lines {
        apply_government_deductions_for_line(
            pool,
            line_id,
            employee_id,
            gross_cents,
            settings.pay_period,
            toggles,
            &type_map,
            compensation_map.get(&employee_id),
        )
        .await?;
    }

    recalculate_lwop_deductions_for_run(pool, run_id).await
}

/// Recomputes auto-calculated LWOP deductions for every line in a draft run.
pub async fn recalculate_lwop_deductions_for_run(pool: &PgPool, run_id: Uuid) -> AppResult<()> {
    let run = get_run(pool, run_id).await?;
    if run.status != PayrollRunStatus::Draft {
        return Err(AppError::bad_request(
            "LWOP deductions can only be recalculated on draft payroll runs",
        ));
    }

    let lwop_type_id = lwop_type_id(pool).await?;
    let lines: Vec<(Uuid, Uuid, i64, i32)> = sqlx::query_as(
        "SELECT l.id, l.employee_id, l.gross_pay_cents, l.lwop_days FROM payroll_lines l WHERE l.run_id = $1",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let employee_ids: Vec<Uuid> = lines.iter().map(|(_, eid, _, _)| *eid).collect();
    let compensation_map = get_compensation_map_as_of(pool, &employee_ids, run.period_end).await?;

    for (line_id, employee_id, gross_cents, lwop_days) in lines {
        apply_lwop_deduction_for_line(
            pool,
            line_id,
            employee_id,
            gross_cents,
            lwop_days,
            lwop_type_id,
            compensation_map.get(&employee_id),
        )
        .await?;
    }

    refresh_all_line_net_pay(pool, run_id).await
}

#[derive(Debug, Clone, Default)]
pub struct FinalizePreflight {
    pub stale_government_count: usize,
    pub missing_government_count: usize,
    pub stale_lwop_count: usize,
    pub missing_lwop_count: usize,
    pub over_gross_count: usize,
    pub disabled_auto_remaining_count: usize,
}

impl FinalizePreflight {
    pub fn has_blocking_issues(&self) -> bool {
        self.over_gross_count > 0
    }

    pub fn has_warnings(&self) -> bool {
        self.stale_government_count > 0
            || self.missing_government_count > 0
            || self.stale_lwop_count > 0
            || self.missing_lwop_count > 0
            || self.disabled_auto_remaining_count > 0
    }

    pub fn has_lwop_warnings(&self) -> bool {
        self.stale_lwop_count > 0 || self.missing_lwop_count > 0
    }
}

pub async fn preflight_finalize_run(
    pool: &PgPool,
    run_id: Uuid,
    settings: &CompanySettings,
) -> AppResult<FinalizePreflight> {
    let run = get_run(pool, run_id).await?;
    let toggles = load_government_toggles(pool).await?;
    let type_map = government_type_ids(pool).await?;
    let lwop_type_id = lwop_type_id(pool).await?;

    let lines: Vec<(Uuid, Uuid, i64, i32)> = sqlx::query_as(
        "SELECT l.id, l.employee_id, l.gross_pay_cents, l.lwop_days FROM payroll_lines l WHERE l.run_id = $1",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let employee_ids: Vec<Uuid> = lines.iter().map(|(_, eid, _, _)| *eid).collect();
    let compensation_map = get_compensation_map_as_of(pool, &employee_ids, run.period_end).await?;

    let mut result = FinalizePreflight::default();

    for (line_id, employee_id, gross_cents, lwop_days) in lines {
        let total_deductions: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(amount_cents), 0) FROM payroll_deductions WHERE line_id = $1",
        )
        .bind(line_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if total_deductions > gross_cents {
            result.over_gross_count += 1;
        }

        let existing: Vec<(String, i64, Option<String>)> = sqlx::query_as(
            "SELECT t.code, d.amount_cents, d.note
             FROM payroll_deductions d
             JOIN deduction_types t ON t.id = d.deduction_type_id
             WHERE d.line_id = $1",
        )
        .bind(line_id)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        let expected = compensation_map.get(&employee_id).map(|comp| {
            period_government_deductions_cents(comp.monthly_salary_cents, settings.pay_period)
        });

        for (code, _amount, note) in &existing {
            if !is_government_deduction_code(code) {
                continue;
            }
            let is_auto = note.as_deref() == Some(AUTO_NOTE);
            let enabled = toggles.is_enabled_for_code(code);
            if is_auto && !enabled {
                result.disabled_auto_remaining_count += 1;
            }
            if is_auto && enabled {
                if let Some(exp) = &expected {
                    let want = exp.for_code(code);
                    if type_map.contains_key(code.as_str()) && _amount != &want {
                        result.stale_government_count += 1;
                    }
                }
            }
        }

        if toggles.any_enabled() {
            if let Some(exp) = &expected {
                for code in [GOV_CODE_SSS, GOV_CODE_PHIC, GOV_CODE_HDMF, GOV_CODE_WHT] {
                    if !toggles.is_enabled_for_code(code) || !type_map.contains_key(code) {
                        continue;
                    }
                    let want = exp.for_code(code);
                    if want <= 0 {
                        continue;
                    }
                    let has_auto = existing.iter().any(|(c, a, n)| {
                        c == code && n.as_deref() == Some(AUTO_NOTE) && *a == want
                    });
                    if !has_auto {
                        result.missing_government_count += 1;
                    }
                }
            }
        }

        if lwop_days > 0 {
            if let (Some(_), Some(comp)) = (lwop_type_id, compensation_map.get(&employee_id)) {
                let want = lwop_deduction_cents(comp.monthly_salary_cents, lwop_days as i64);
                if want > 0 {
                    let has_auto = existing.iter().any(|(c, a, n)| {
                        c == LWOP_CODE && n.as_deref() == Some(AUTO_NOTE) && *a == want
                    });
                    if !has_auto {
                        result.missing_lwop_count += 1;
                    }
                    for (code, amount, note) in &existing {
                        if code == LWOP_CODE
                            && note.as_deref() == Some(AUTO_NOTE)
                            && *amount != want
                        {
                            result.stale_lwop_count += 1;
                            break;
                        }
                    }
                }
            }
        } else {
            let orphan_auto = existing
                .iter()
                .any(|(c, _, n)| c == LWOP_CODE && n.as_deref() == Some(AUTO_NOTE));
            if orphan_auto {
                result.stale_lwop_count += 1;
            }
        }

        let _ = gross_cents;
    }

    let _ = run;
    Ok(result)
}

async fn apply_employee_defaults_for_line(
    pool: &PgPool,
    line_id: Uuid,
    employee_id: Uuid,
    gross_cents: i64,
) -> AppResult<()> {
    let existing: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM payroll_deductions WHERE line_id = $1")
            .bind(line_id)
            .fetch_one(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    if existing > 0 {
        return Ok(());
    }

    let defaults: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT d.deduction_type_id, d.amount_cents
         FROM employee_deduction_defaults d
         JOIN deduction_types t ON t.id = d.deduction_type_id AND t.is_active = TRUE
         WHERE d.employee_id = $1
           AND d.amount_cents > 0
           AND t.code NOT IN ('SSS', 'PHIC', 'HDMF', 'WHT', 'LWOP')",
    )
    .bind(employee_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if defaults.is_empty() {
        return Ok(());
    }

    let total: i64 = defaults.iter().map(|(_, amount)| amount).sum();
    if total > gross_cents {
        return Ok(());
    }

    for (type_id, amount) in defaults {
        sqlx::query(
            "INSERT INTO payroll_deductions (line_id, deduction_type_id, amount_cents)
             VALUES ($1, $2, $3)",
        )
        .bind(line_id)
        .bind(type_id)
        .bind(amount)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn apply_government_deductions_for_line(
    pool: &PgPool,
    line_id: Uuid,
    employee_id: Uuid,
    gross_cents: i64,
    pay_period: PayPeriodType,
    toggles: GovernmentDeductionToggles,
    type_map: &HashMap<String, Uuid>,
    compensation: Option<&crate::models::CompensationProfile>,
) -> AppResult<()> {
    remove_disabled_auto_government_deductions(pool, line_id, toggles).await?;

    if !toggles.any_enabled() {
        return Ok(());
    }

    let Some(comp) = compensation else {
        return Ok(());
    };

    let amounts = period_government_deductions_cents(comp.monthly_salary_cents, pay_period);
    let mut planned: Vec<(Uuid, i64)> = Vec::new();

    for code in [GOV_CODE_SSS, GOV_CODE_PHIC, GOV_CODE_HDMF, GOV_CODE_WHT] {
        if !toggles.is_enabled_for_code(code) {
            continue;
        }
        let Some(type_id) = type_map.get(code).copied() else {
            continue;
        };
        let amount = amounts.for_code(code);
        if amount > 0 {
            planned.push((type_id, amount));
        }
    }

    if planned.is_empty() {
        return Ok(());
    }

    let manual_gov_total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(d.amount_cents), 0)
         FROM payroll_deductions d
         JOIN deduction_types t ON t.id = d.deduction_type_id
         WHERE d.line_id = $1
           AND t.code = ANY($2)
           AND COALESCE(d.note, '') != $3",
    )
    .bind(line_id)
    .bind(&[GOV_CODE_SSS, GOV_CODE_PHIC, GOV_CODE_HDMF, GOV_CODE_WHT][..])
    .bind(AUTO_NOTE)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let existing_non_gov: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(d.amount_cents), 0)
         FROM payroll_deductions d
         JOIN deduction_types t ON t.id = d.deduction_type_id
         WHERE d.line_id = $1 AND t.code NOT IN ('SSS', 'PHIC', 'HDMF', 'WHT')",
    )
    .bind(line_id)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let auto_total: i64 = planned.iter().map(|(_, a)| a).sum();
    if manual_gov_total + existing_non_gov + auto_total > gross_cents {
        tracing::warn!(
            line_id = %line_id,
            employee_id = %employee_id,
            "Skipping government auto-deductions: total would exceed gross pay"
        );
        return Ok(());
    }

    for (type_id, amount) in planned {
        let updated = sqlx::query(
            "UPDATE payroll_deductions d
             SET amount_cents = $3, note = $4
             FROM deduction_types t
             WHERE d.line_id = $1
               AND d.deduction_type_id = t.id
               AND d.deduction_type_id = $2
               AND d.note = $4",
        )
        .bind(line_id)
        .bind(type_id)
        .bind(amount)
        .bind(AUTO_NOTE)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if updated.rows_affected() == 0 {
            sqlx::query(
                "INSERT INTO payroll_deductions (line_id, deduction_type_id, amount_cents, note)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(line_id)
            .bind(type_id)
            .bind(amount)
            .bind(AUTO_NOTE)
            .execute(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        }
    }

    Ok(())
}

async fn remove_disabled_auto_government_deductions(
    pool: &PgPool,
    line_id: Uuid,
    toggles: GovernmentDeductionToggles,
) -> AppResult<()> {
    let disabled: Vec<&str> = [GOV_CODE_SSS, GOV_CODE_PHIC, GOV_CODE_HDMF, GOV_CODE_WHT]
        .into_iter()
        .filter(|code| !toggles.is_enabled_for_code(code))
        .collect();

    if disabled.is_empty() {
        return Ok(());
    }

    sqlx::query(
        "DELETE FROM payroll_deductions d
         USING deduction_types t
         WHERE d.line_id = $1
           AND d.deduction_type_id = t.id
           AND t.code = ANY($2)
           AND d.note = $3",
    )
    .bind(line_id)
    .bind(&disabled)
    .bind(AUTO_NOTE)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

/// Reads government auto-deduction toggles from `company_settings` (migration 025).
pub async fn load_government_toggles(pool: &PgPool) -> AppResult<GovernmentDeductionToggles> {
    let row: (bool, bool, bool, bool) = sqlx::query_as(
        "SELECT auto_deduct_sss, auto_deduct_phic, auto_deduct_hdmf, auto_deduct_wht
         FROM company_settings WHERE id = 1",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(GovernmentDeductionToggles {
        sss: row.0,
        phic: row.1,
        hdmf: row.2,
        wht: row.3,
    })
}

async fn lwop_type_id(pool: &PgPool) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar("SELECT id FROM deduction_types WHERE code = $1 AND is_active = TRUE")
        .bind(LWOP_CODE)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))
}

async fn apply_lwop_deduction_for_line(
    pool: &PgPool,
    line_id: Uuid,
    employee_id: Uuid,
    gross_cents: i64,
    lwop_days: i32,
    lwop_type_id: Option<Uuid>,
    compensation: Option<&crate::models::CompensationProfile>,
) -> AppResult<()> {
    let Some(type_id) = lwop_type_id else {
        return Ok(());
    };

    if lwop_days <= 0 {
        sqlx::query(
            "DELETE FROM payroll_deductions d
             USING deduction_types t
             WHERE d.line_id = $1
               AND d.deduction_type_id = t.id
               AND t.code = $2
               AND d.note = $3",
        )
        .bind(line_id)
        .bind(LWOP_CODE)
        .bind(AUTO_NOTE)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        return Ok(());
    }

    let Some(comp) = compensation else {
        return Ok(());
    };

    let amount = lwop_deduction_cents(comp.monthly_salary_cents, lwop_days as i64);
    if amount <= 0 {
        return Ok(());
    }

    let other_total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(d.amount_cents), 0)
         FROM payroll_deductions d
         JOIN deduction_types t ON t.id = d.deduction_type_id
         WHERE d.line_id = $1
           AND NOT (t.code = $2 AND COALESCE(d.note, '') = $3)",
    )
    .bind(line_id)
    .bind(LWOP_CODE)
    .bind(AUTO_NOTE)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if other_total + amount > gross_cents {
        tracing::warn!(
            line_id = %line_id,
            employee_id = %employee_id,
            "Skipping LWOP auto-deduction: total would exceed gross pay"
        );
        return Ok(());
    }

    let updated = sqlx::query(
        "UPDATE payroll_deductions d
         SET amount_cents = $3, note = $4
         FROM deduction_types t
         WHERE d.line_id = $1
           AND d.deduction_type_id = t.id
           AND d.deduction_type_id = $2
           AND d.note = $4",
    )
    .bind(line_id)
    .bind(type_id)
    .bind(amount)
    .bind(AUTO_NOTE)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if updated.rows_affected() == 0 {
        sqlx::query(
            "INSERT INTO payroll_deductions (line_id, deduction_type_id, amount_cents, note)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(line_id)
        .bind(type_id)
        .bind(amount)
        .bind(AUTO_NOTE)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    }

    Ok(())
}

async fn government_type_ids(pool: &PgPool) -> AppResult<HashMap<String, Uuid>> {
    let rows: Vec<(String, Uuid)> = sqlx::query_as(
        "SELECT code, id FROM deduction_types
         WHERE code IN ('SSS', 'PHIC', 'HDMF', 'WHT') AND is_active = TRUE",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::FinalizePreflight;

    #[test]
    fn preflight_blocking_only_on_over_gross() {
        let clean = FinalizePreflight::default();
        assert!(!clean.has_blocking_issues());
        assert!(!clean.has_warnings());

        let stale = FinalizePreflight {
            stale_government_count: 2,
            ..Default::default()
        };
        assert!(!stale.has_blocking_issues());
        assert!(stale.has_warnings());

        let over = FinalizePreflight {
            over_gross_count: 1,
            ..Default::default()
        };
        assert!(over.has_blocking_issues());

        let lwop_stale = FinalizePreflight {
            stale_lwop_count: 1,
            ..Default::default()
        };
        assert!(lwop_stale.has_lwop_warnings());
        assert!(lwop_stale.has_warnings());
    }
}
