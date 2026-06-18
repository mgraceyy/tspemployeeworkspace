use std::collections::HashMap;

use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{CompensationHistoryRow, CompensationProfile};

pub fn parse_salary_to_cents(input: &str) -> AppResult<i64> {
    crate::services::money::parse_money_to_cents(input, false).map_err(|error| match error {
        AppError::BadRequest(message) if message == "Amount is required" => {
            AppError::bad_request("Monthly salary is required")
        }
        AppError::BadRequest(message) if message == "Amount must be greater than zero" => {
            AppError::bad_request("Monthly salary must be greater than zero")
        }
        other => other,
    })
}

pub fn parse_allowance_to_cents(input: &str) -> AppResult<i64> {
    crate::services::money::parse_money_to_cents(input, true)
}

pub fn format_salary_cents(cents: i64) -> String {
    let whole = cents / 100;
    let frac = (cents % 100).unsigned_abs();
    format!("{whole}.{frac:02}")
}

const PROFILE_COLUMNS: &str =
    "employee_id, monthly_salary_cents, ot_rate_percent, transport_allowance_cents, meal_allowance_cents, effective_from";

pub async fn get_compensation(
    pool: &PgPool,
    employee_id: Uuid,
) -> AppResult<Option<CompensationProfile>> {
    sqlx::query_as::<_, CompensationProfile>(&format!(
        "SELECT {PROFILE_COLUMNS} FROM compensation_profiles WHERE employee_id = $1"
    ))
    .bind(employee_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn get_compensation_map_as_of(
    pool: &PgPool,
    employee_ids: &[Uuid],
    as_of: Date,
) -> AppResult<HashMap<Uuid, CompensationProfile>> {
    if employee_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let current = sqlx::query_as::<_, CompensationProfile>(&format!(
        "SELECT {PROFILE_COLUMNS} FROM compensation_profiles WHERE employee_id = ANY($1)"
    ))
    .bind(employee_ids)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let mut map = HashMap::with_capacity(employee_ids.len());
    let mut need_history = Vec::new();
    for id in employee_ids {
        if let Some(profile) = current.iter().find(|p| p.employee_id == *id) {
            if profile.effective_from <= as_of {
                map.insert(*id, profile.clone());
                continue;
            }
        }
        need_history.push(*id);
    }

    if !need_history.is_empty() {
        let history = sqlx::query_as::<_, CompensationProfile>(
            "SELECT DISTINCT ON (employee_id) employee_id, monthly_salary_cents, ot_rate_percent,
                    transport_allowance_cents, meal_allowance_cents, effective_from
             FROM compensation_history
             WHERE employee_id = ANY($1)
               AND effective_from <= $2
               AND (effective_to IS NULL OR effective_to >= $2)
             ORDER BY employee_id, effective_from DESC",
        )
        .bind(&need_history)
        .bind(as_of)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        for profile in history {
            map.insert(profile.employee_id, profile);
        }
    }

    Ok(map)
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
        "SELECT employee_id, monthly_salary_cents, ot_rate_percent, transport_allowance_cents,
                meal_allowance_cents, effective_from
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

pub async fn list_history(
    pool: &PgPool,
    employee_id: Uuid,
) -> AppResult<Vec<CompensationHistoryRow>> {
    sqlx::query_as::<_, CompensationHistoryRow>(
        "SELECT id, employee_id, monthly_salary_cents, ot_rate_percent, transport_allowance_cents,
                meal_allowance_cents, effective_from, effective_to, created_at
         FROM compensation_history
         WHERE employee_id = $1
         ORDER BY effective_from DESC, created_at DESC",
    )
    .bind(employee_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

#[derive(Debug, Clone)]
pub struct UpsertProfileInput {
    pub employee_id: Uuid,
    pub monthly_salary_cents: i64,
    pub ot_rate_percent: i32,
    pub transport_allowance_cents: i64,
    pub meal_allowance_cents: i64,
    pub effective_from: Date,
    pub updated_by: Uuid,
}

impl UpsertProfileInput {
    pub fn new(
        employee_id: Uuid,
        monthly_salary_cents: i64,
        effective_from: Date,
        updated_by: Uuid,
    ) -> Self {
        Self {
            employee_id,
            monthly_salary_cents,
            ot_rate_percent: 132,
            transport_allowance_cents: 0,
            meal_allowance_cents: 0,
            effective_from,
            updated_by,
        }
    }
}

pub async fn upsert_profile(pool: &PgPool, input: &UpsertProfileInput) -> AppResult<()> {
    if !(100..=300).contains(&input.ot_rate_percent) {
        return Err(AppError::bad_request(
            "OT rate must be between 100% and 300%",
        ));
    }
    if input.transport_allowance_cents < 0 || input.meal_allowance_cents < 0 {
        return Err(AppError::bad_request("Allowances cannot be negative"));
    }

    let existing = get_compensation(pool, input.employee_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    if let Some(old) = existing {
        let closes_on = input.effective_from - time::Duration::days(1);
        if closes_on >= old.effective_from {
            sqlx::query(
                "INSERT INTO compensation_history
                    (employee_id, monthly_salary_cents, ot_rate_percent, transport_allowance_cents,
                     meal_allowance_cents, effective_from, effective_to, changed_by)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(input.employee_id)
            .bind(old.monthly_salary_cents)
            .bind(old.ot_rate_percent)
            .bind(old.transport_allowance_cents)
            .bind(old.meal_allowance_cents)
            .bind(old.effective_from)
            .bind(closes_on)
            .bind(input.updated_by)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        }
    }

    sqlx::query(
        "INSERT INTO compensation_profiles
            (employee_id, monthly_salary_cents, ot_rate_percent, transport_allowance_cents,
             meal_allowance_cents, effective_from, updated_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (employee_id) DO UPDATE SET
            monthly_salary_cents = EXCLUDED.monthly_salary_cents,
            ot_rate_percent = EXCLUDED.ot_rate_percent,
            transport_allowance_cents = EXCLUDED.transport_allowance_cents,
            meal_allowance_cents = EXCLUDED.meal_allowance_cents,
            effective_from = EXCLUDED.effective_from,
            updated_by = EXCLUDED.updated_by,
            updated_at = now()",
    )
    .bind(input.employee_id)
    .bind(input.monthly_salary_cents)
    .bind(input.ot_rate_percent)
    .bind(input.transport_allowance_cents)
    .bind(input.meal_allowance_cents)
    .bind(input.effective_from)
    .bind(input.updated_by)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct DeductionDefaultInput {
    pub deduction_type_id: Uuid,
    pub amount_cents: i64,
}

pub async fn list_deduction_defaults(
    pool: &PgPool,
    employee_id: Uuid,
) -> AppResult<Vec<(Uuid, i64)>> {
    let rows: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT deduction_type_id, amount_cents
         FROM employee_deduction_defaults
         WHERE employee_id = $1",
    )
    .bind(employee_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn save_deduction_defaults(
    pool: &PgPool,
    employee_id: Uuid,
    editor_id: Uuid,
    inputs: &[DeductionDefaultInput],
) -> AppResult<()> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    sqlx::query("DELETE FROM employee_deduction_defaults WHERE employee_id = $1")
        .bind(employee_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    for input in inputs {
        if input.amount_cents <= 0 {
            continue;
        }
        sqlx::query(
            "INSERT INTO employee_deduction_defaults
                (employee_id, deduction_type_id, amount_cents, updated_by)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(employee_id)
        .bind(input.deduction_type_id)
        .bind(input.amount_cents)
        .bind(editor_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    }

    tx.commit()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}