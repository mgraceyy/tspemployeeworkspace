use sqlx::PgPool;
use time::Time;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::ShiftTemplate;

pub async fn list_for_employee(pool: &PgPool, employee_id: Uuid) -> AppResult<Vec<ShiftTemplate>> {
    let shifts = sqlx::query_as::<_, ShiftTemplate>(
        "SELECT id, employee_id, day_of_week, start_time, end_time
         FROM shift_templates
         WHERE employee_id = $1
         ORDER BY day_of_week",
    )
    .bind(employee_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(shifts)
}

pub async fn upsert_shift(
    pool: &PgPool,
    employee_id: Uuid,
    day_of_week: i16,
    start_time: Time,
    end_time: Time,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO shift_templates (employee_id, day_of_week, start_time, end_time)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (employee_id, day_of_week)
         DO UPDATE SET start_time = EXCLUDED.start_time, end_time = EXCLUDED.end_time",
    )
    .bind(employee_id)
    .bind(day_of_week)
    .bind(start_time)
    .bind(end_time)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}