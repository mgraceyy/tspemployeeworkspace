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
    let trimmed = input.trim().replace(',', "");
    if trimmed.is_empty() {
        return Ok(0);
    }
    let amount: f64 = trimmed
        .parse()
        .map_err(|_| AppError::bad_request("Amount must be a valid number"))?;
    if amount < 0.0 {
        return Err(AppError::bad_request("Amount cannot be negative"));
    }
    if amount == 0.0 {
        return Ok(0);
    }
    if amount > 99_999_999.99 {
        return Err(AppError::bad_request("Amount is too large"));
    }
    Ok((amount * 100.0).round() as i64)
}

pub async fn list_deduction_types(pool: &PgPool) -> AppResult<Vec<DeductionType>> {
    sqlx::query_as::<_, DeductionType>("SELECT id, code, name FROM deduction_types ORDER BY code")
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))
}

pub async fn get_line_for_run(
    pool: &PgPool,
    run_id: Uuid,
    line_id: Uuid,
) -> AppResult<crate::models::PayrollLineWithEmployee> {
    sqlx::query_as::<_, crate::models::PayrollLineWithEmployee>(
        "SELECT l.id, l.employee_id, e.employee_code, e.full_name, p.department,
                l.regular_minutes, l.approved_ot_minutes, l.pending_ot_minutes, l.no_show_days,
                l.base_pay_cents, l.no_show_deduction_cents, l.ot_pay_cents,
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

    for input in inputs {
        if input.amount_cents < 0 {
            return Err(AppError::bad_request(
                "Deduction amounts cannot be negative",
            ));
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
