use sqlx::PgPool;
use uuid::Uuid;

use super::runs::get_run;
use crate::error::{AppError, AppResult};
use crate::models::{DeductionType, PayrollDeductionWithType, PayrollRunStatus};

#[derive(Debug, Clone)]
pub struct DeductionInput {
    pub deduction_type_id: Uuid,
    pub amount_cents: i64,
    pub note: Option<String>,
}

pub fn parse_optional_amount_to_cents(input: &str) -> AppResult<i64> {
    crate::services::money::parse_money_to_cents(input, true)
}

const MAX_DEDUCTION_NOTE_LEN: usize = 200;

pub async fn list_deduction_types(pool: &PgPool) -> AppResult<Vec<DeductionType>> {
    sqlx::query_as::<_, DeductionType>(
        "SELECT id, code, name, is_active, sort_order
         FROM deduction_types
         WHERE is_active = TRUE
         ORDER BY sort_order, code",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn list_all_deduction_types(pool: &PgPool) -> AppResult<Vec<DeductionType>> {
    sqlx::query_as::<_, DeductionType>(
        "SELECT id, code, name, is_active, sort_order
         FROM deduction_types
         ORDER BY sort_order, code",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn create_deduction_type(
    pool: &PgPool,
    code: &str,
    name: &str,
) -> AppResult<DeductionType> {
    let code = code.trim().to_uppercase();
    let name = name.trim();
    if code.is_empty() || name.is_empty() {
        return Err(AppError::bad_request("Code and name are required"));
    }
    sqlx::query_as::<_, DeductionType>(
        "INSERT INTO deduction_types (code, name, sort_order)
         VALUES ($1, $2, (SELECT COALESCE(MAX(sort_order), 0) + 10 FROM deduction_types))
         RETURNING id, code, name, is_active, sort_order",
    )
    .bind(&code)
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(db) = &e {
            if db.constraint() == Some("deduction_types_code_key") {
                return AppError::bad_request("Deduction code already exists");
            }
        }
        AppError::Internal(e.into())
    })
}

pub async fn set_deduction_type_active(
    pool: &PgPool,
    type_id: Uuid,
    is_active: bool,
) -> AppResult<()> {
    let updated = sqlx::query("UPDATE deduction_types SET is_active = $2 WHERE id = $1")
        .bind(type_id)
        .bind(is_active)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn apply_deduction_defaults_for_run(pool: &PgPool, run_id: Uuid) -> AppResult<()> {
    let lines = sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT l.id, l.employee_id FROM payroll_lines l WHERE l.run_id = $1",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    for (line_id, employee_id) in lines {
        let existing: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM payroll_deductions WHERE line_id = $1")
                .bind(line_id)
                .fetch_one(pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        if existing > 0 {
            continue;
        }

        let defaults: Vec<(Uuid, i64)> = sqlx::query_as(
            "SELECT d.deduction_type_id, d.amount_cents
             FROM employee_deduction_defaults d
             JOIN deduction_types t ON t.id = d.deduction_type_id AND t.is_active = TRUE
             WHERE d.employee_id = $1 AND d.amount_cents > 0",
        )
        .bind(employee_id)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if defaults.is_empty() {
            continue;
        }

        let gross: i64 =
            sqlx::query_scalar("SELECT gross_pay_cents FROM payroll_lines WHERE id = $1")
                .bind(line_id)
                .fetch_one(pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        let total: i64 = defaults.iter().map(|(_, amount)| amount).sum();
        if total > gross {
            continue;
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
        let net = gross - total;
        sqlx::query("UPDATE payroll_lines SET net_pay_cents = $2 WHERE id = $1")
            .bind(line_id)
            .bind(net)
            .execute(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    }
    Ok(())
}

pub async fn get_line_for_run(
    pool: &PgPool,
    run_id: Uuid,
    line_id: Uuid,
) -> AppResult<crate::models::PayrollLineWithEmployee> {
    sqlx::query_as::<_, crate::models::PayrollLineWithEmployee>(
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
         WHERE l.run_id = $1 AND l.id = $2",
    )
    .bind(run_id)
    .bind(line_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::NotFound)
}

pub async fn list_deductions_for_line(
    pool: &PgPool,
    line_id: Uuid,
) -> AppResult<Vec<PayrollDeductionWithType>> {
    sqlx::query_as::<_, PayrollDeductionWithType>(
        "SELECT d.id, d.line_id, d.deduction_type_id, t.code, t.name, d.amount_cents, d.note
         FROM payroll_deductions d
         JOIN deduction_types t ON t.id = d.deduction_type_id
         WHERE d.line_id = $1
         ORDER BY t.code",
    )
    .bind(line_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn save_line_deductions(
    pool: &PgPool,
    run_id: Uuid,
    line_id: Uuid,
    inputs: &[DeductionInput],
) -> AppResult<()> {
    let run = get_run(pool, run_id).await?;
    if run.status != PayrollRunStatus::Draft {
        return Err(AppError::bad_request(
            "Deductions can only be edited on draft payroll runs",
        ));
    }

    let line = get_line_for_run(pool, run_id, line_id).await?;
    let total_deductions: i64 = inputs.iter().map(|i| i.amount_cents).sum();
    if total_deductions > line.gross_pay_cents {
        return Err(AppError::bad_request(
            "Total deductions cannot exceed gross pay for this employee",
        ));
    }

    let valid_type_ids: std::collections::HashSet<Uuid> =
        sqlx::query_scalar("SELECT id FROM deduction_types")
            .fetch_all(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?
            .into_iter()
            .collect();

    for input in inputs {
        if input.amount_cents < 0 {
            return Err(AppError::bad_request(
                "Deduction amounts cannot be negative",
            ));
        }
        if input.amount_cents > 0 && !valid_type_ids.contains(&input.deduction_type_id) {
            return Err(AppError::bad_request("Unknown deduction type"));
        }
        if let Some(ref note) = input.note {
            if note.chars().count() > MAX_DEDUCTION_NOTE_LEN {
                return Err(AppError::bad_request(format!(
                    "Deduction notes cannot exceed {MAX_DEDUCTION_NOTE_LEN} characters"
                )));
            }
        }
    }

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    sqlx::query("DELETE FROM payroll_deductions WHERE line_id = $1")
        .bind(line_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    for input in inputs {
        if input.amount_cents == 0 {
            continue;
        }
        sqlx::query(
            "INSERT INTO payroll_deductions (line_id, deduction_type_id, amount_cents, note)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(line_id)
        .bind(input.deduction_type_id)
        .bind(input.amount_cents)
        .bind(
            input
                .note
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty()),
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    }

    let net_pay = line.gross_pay_cents - total_deductions;
    sqlx::query("UPDATE payroll_lines SET net_pay_cents = $2 WHERE id = $1")
        .bind(line_id)
        .bind(net_pay)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

pub async fn refresh_all_line_net_pay(pool: &PgPool, run_id: Uuid) -> AppResult<()> {
    sqlx::query(
        "UPDATE payroll_lines l
         SET net_pay_cents = l.gross_pay_cents - COALESCE((
             SELECT SUM(d.amount_cents) FROM payroll_deductions d WHERE d.line_id = l.id
         ), 0)
         WHERE l.run_id = $1",
    )
    .bind(run_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_optional_amount_to_cents;

    #[test]
    fn empty_amount_is_zero() {
        assert_eq!(parse_optional_amount_to_cents("").unwrap(), 0);
        assert_eq!(parse_optional_amount_to_cents("  ").unwrap(), 0);
    }

    #[test]
    fn parses_decimal_amount() {
        assert_eq!(parse_optional_amount_to_cents("1,234.50").unwrap(), 123_450);
    }
}
