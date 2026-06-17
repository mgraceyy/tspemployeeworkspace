use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::CompanyHoliday;

pub async fn is_holiday(pool: &PgPool, date: Date) -> AppResult<bool> {
    let exists: Option<bool> =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM company_holidays WHERE holiday_date = $1)")
            .bind(date)
            .fetch_one(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    Ok(exists.unwrap_or(false))
}

pub async fn list_holidays(pool: &PgPool) -> AppResult<Vec<CompanyHoliday>> {
    let rows = sqlx::query_as::<_, CompanyHoliday>(
        "SELECT id, holiday_date, name FROM company_holidays ORDER BY holiday_date",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn list_holidays_between(
    pool: &PgPool,
    start: Date,
    end: Date,
) -> AppResult<Vec<CompanyHoliday>> {
    let rows = sqlx::query_as::<_, CompanyHoliday>(
        "SELECT id, holiday_date, name
         FROM company_holidays
         WHERE holiday_date BETWEEN $1 AND $2
         ORDER BY holiday_date",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn add_holiday(
    pool: &PgPool,
    holiday_date: Date,
    name: &str,
) -> AppResult<CompanyHoliday> {
    let row = sqlx::query_as::<_, CompanyHoliday>(
        "INSERT INTO company_holidays (holiday_date, name)
         VALUES ($1, $2)
         RETURNING id, holiday_date, name",
    )
    .bind(holiday_date)
    .bind(name.trim())
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(row)
}

pub async fn delete_holiday(pool: &PgPool, holiday_id: Uuid) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM company_holidays WHERE id = $1")
        .bind(holiday_id)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}
