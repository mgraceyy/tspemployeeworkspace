use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::CompensationProfile;

pub fn parse_salary_to_cents(input: &str) -> AppResult<i64> {
    let trimmed = input.trim().replace(',', "");
    if trimmed.is_empty() {
        return Err(AppError::bad_request("Monthly salary is required"));
    }
    let amount: f64 = trimmed
        .parse()
        .map_err(|_| AppError::bad_request("Monthly salary must be a valid number"))?;
    if amount <= 0.0 {
        return Err(AppError::bad_request(
            "Monthly salary must be greater than zero",
        ));
    }
    if amount > 99_999_999.99 {
        return Err(AppError::bad_request("Monthly salary is too large"));
    }
    Ok((amount * 100.0).round() as i64)
}

pub fn format_salary_cents(cents: i64) -> String {
    let whole = cents / 100;
    let frac = (cents % 100).unsigned_abs();
    format!("{whole}.{frac:02}")
}

pub async fn get_compensation(
    pool: &PgPool,
    employee_id: Uuid,
) -> AppResult<Option<CompensationProfile>> {
    sqlx::query_as::<_, CompensationProfile>(
        "SELECT employee_id, monthly_salary_cents, ot_rate_percent, effective_from
         FROM compensation_profiles
         WHERE employee_id = $1",
    )
    .bind(employee_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn get_compensation_as_of(
    pool: &PgPool,
    employee_id: Uuid,
    as_of: Date,
) -> AppResult<Option<CompensationProfile>> {
    let current = get_compensation(pool, employee_id).await?;
    if let Some(ref profile) = current {
        if profile.effective_from <= as_of {
            return Ok(current);
        }
    }

    sqlx::query_as::<_, CompensationProfile>(
        "SELECT employee_id, monthly_salary_cents, ot_rate_percent, effective_from
         FROM compensation_history
         WHERE employee_id = $1
           AND effective_from <= $2
           AND (effective_to IS NULL OR effective_to >= $2)
         ORDER BY effective_from DESC
         LIMIT 1",
    )
    .bind(employee_id)
    .bind(as_of)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn upsert_profile(
    pool: &PgPool,
    employee_id: Uuid,
    monthly_salary_cents: i64,
    ot_rate_percent: i32,
    effective_from: Date,
    updated_by: Uuid,
) -> AppResult<()> {
    if !(100..=300).contains(&ot_rate_percent) {
        return Err(AppError::bad_request(
            "OT rate must be between 100% and 300%",
        ));
    }

    let existing = get_compensation(pool, employee_id).await?;
    if let Some(old) = existing {
        let closes_on = effective_from - time::Duration::days(1);
        if closes_on >= old.effective_from {
            sqlx::query(
                "INSERT INTO compensation_history
                    (employee_id, monthly_salary_cents, ot_rate_percent, effective_from, effective_to, changed_by)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(employee_id)
            .bind(old.monthly_salary_cents)
            .bind(old.ot_rate_percent)
            .bind(old.effective_from)
            .bind(closes_on)
            .bind(updated_by)
            .execute(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        }
    }

    sqlx::query(
        "INSERT INTO compensation_profiles
            (employee_id, monthly_salary_cents, ot_rate_percent, effective_from, updated_by)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (employee_id) DO UPDATE SET
            monthly_salary_cents = EXCLUDED.monthly_salary_cents,
            ot_rate_percent = EXCLUDED.ot_rate_percent,
            effective_from = EXCLUDED.effective_from,
            updated_by = EXCLUDED.updated_by,
            updated_at = now()",
    )
    .bind(employee_id)
    .bind(monthly_salary_cents)
    .bind(ot_rate_percent)
    .bind(effective_from)
    .bind(updated_by)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(())
}
