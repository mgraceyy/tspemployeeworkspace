use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::UserRole;
use crate::services::timezone::format_date;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ReportPreset {
    pub id: Uuid,
    pub name: String,
    pub department: Option<String>,
    pub role: Option<UserRole>,
    pub employee_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
}

pub async fn list_report_presets(pool: &PgPool) -> AppResult<Vec<ReportPreset>> {
    let rows = sqlx::query_as::<_, ReportPreset>(
        "SELECT id, name, department, role, employee_id, created_at
         FROM report_presets
         ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn create_report_preset(
    pool: &PgPool,
    name: &str,
    department: Option<&str>,
    role: Option<UserRole>,
    employee_id: Option<Uuid>,
    created_by: Uuid,
) -> AppResult<ReportPreset> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::bad_request("Preset name is required"));
    }
    let department = department
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let preset = sqlx::query_as::<_, ReportPreset>(
        "INSERT INTO report_presets (name, department, role, employee_id, created_by)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, name, department, role, employee_id, created_at",
    )
    .bind(trimmed)
    .bind(department)
    .bind(role)
    .bind(employee_id)
    .bind(created_by)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(db_err) = &e {
            if db_err.constraint() == Some("report_presets_name_key") {
                return AppError::bad_request("A preset with this name already exists");
            }
        }
        AppError::Internal(e.into())
    })?;
    Ok(preset)
}

pub async fn delete_report_preset(pool: &PgPool, preset_id: Uuid) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM report_presets WHERE id = $1")
        .bind(preset_id)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn is_work_date_closed(pool: &PgPool, work_date: Date) -> AppResult<bool> {
    let closed: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM closed_pay_periods
            WHERE $1 BETWEEN period_start AND period_end
         )",
    )
    .bind(work_date)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(closed)
}

pub async fn is_period_closed(pool: &PgPool, start: Date, end: Date) -> AppResult<bool> {
    let closed: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM closed_pay_periods
            WHERE period_start <= $2 AND period_end >= $1
         )",
    )
    .bind(start)
    .bind(end)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(closed)
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClosedPayPeriod {
    pub period_start: Date,
    pub period_end: Date,
    pub note: Option<String>,
}

pub async fn list_overlapping_closed_periods(
    pool: &PgPool,
    start: Date,
    end: Date,
) -> AppResult<Vec<ClosedPayPeriod>> {
    let rows = sqlx::query_as::<_, ClosedPayPeriod>(
        "SELECT period_start, period_end, note
         FROM closed_pay_periods
         WHERE period_start <= $2 AND period_end >= $1
         ORDER BY period_start, period_end",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn is_period_exactly_closed(pool: &PgPool, start: Date, end: Date) -> AppResult<bool> {
    let closed: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM closed_pay_periods
            WHERE period_start = $1 AND period_end = $2
         )",
    )
    .bind(start)
    .bind(end)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(closed)
}

pub async fn assert_work_date_editable(pool: &PgPool, work_date: Date) -> AppResult<()> {
    if is_work_date_closed(pool, work_date).await? {
        return Err(AppError::bad_request(
            "This work date falls in a closed pay period and cannot be changed",
        ));
    }
    Ok(())
}

pub async fn assert_date_range_editable(pool: &PgPool, start: Date, end: Date) -> AppResult<()> {
    if end < start {
        return Err(AppError::bad_request(
            "End date must be on or after start date",
        ));
    }
    if is_period_closed(pool, start, end).await? {
        return Err(AppError::bad_request(
            "One or more dates in this range fall in a closed pay period and cannot be changed",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosePayPeriodResult {
    Closed,
    AlreadyClosed,
}

pub async fn close_pay_period(
    pool: &PgPool,
    start: Date,
    end: Date,
    closed_by: Uuid,
    note: Option<&str>,
) -> AppResult<ClosePayPeriodResult> {
    if end < start {
        return Err(AppError::bad_request(
            "End date must be on or after start date",
        ));
    }

    if is_period_exactly_closed(pool, start, end).await? {
        return Ok(ClosePayPeriodResult::AlreadyClosed);
    }

    let overlaps = list_overlapping_closed_periods(pool, start, end).await?;
    if !overlaps.is_empty() {
        let ranges = overlaps
            .iter()
            .map(|period| {
                format!(
                    "{} to {}",
                    format_date(period.period_start),
                    format_date(period.period_end)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(AppError::bad_request(format!(
            "Cannot close this range — it overlaps existing closed period(s): {ranges}. Reopen the overlapping range(s) first or choose dates that do not overlap."
        )));
    }

    let result = sqlx::query(
        "INSERT INTO closed_pay_periods (period_start, period_end, closed_by, note)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (period_start, period_end) DO NOTHING",
    )
    .bind(start)
    .bind(end)
    .bind(closed_by)
    .bind(note.map(str::trim).filter(|value| !value.is_empty()))
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    if result.rows_affected() == 0 {
        return Ok(ClosePayPeriodResult::AlreadyClosed);
    }
    Ok(ClosePayPeriodResult::Closed)
}

pub async fn assert_payroll_run_allows_reopen(
    pool: &PgPool,
    start: Date,
    end: Date,
) -> AppResult<()> {
    let status: Option<String> = sqlx::query_scalar(
        "SELECT status::text FROM payroll_runs
         WHERE period_start = $1 AND period_end = $2 AND status IN ('draft', 'finalized')
         LIMIT 1",
    )
    .bind(start)
    .bind(end)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if let Some(status) = status {
        let hint = if status == "draft" {
            "Void the draft payroll run first"
        } else {
            "Payroll is already finalized for this period"
        };
        return Err(AppError::bad_request(format!(
            "Cannot reopen this pay period — a {status} payroll run exists. {hint}."
        )));
    }
    Ok(())
}

pub async fn reopen_pay_period(pool: &PgPool, start: Date, end: Date) -> AppResult<()> {
    assert_payroll_run_allows_reopen(pool, start, end).await?;

    let result =
        sqlx::query("DELETE FROM closed_pay_periods WHERE period_start = $1 AND period_end = $2")
            .bind(start)
            .bind(end)
            .execute(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    if result.rows_affected() == 0 {
        return Err(AppError::bad_request("This pay period is not closed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserRole;
    use crate::services::employees::create_employee;
    use time::Month;

    async fn test_admin(pool: &PgPool) -> (Uuid, String) {
        let code = format!("PCAD{}", &Uuid::new_v4().simple().to_string()[..8]);
        let admin = create_employee(
            pool,
            &code,
            "Payroll Controls Test Admin",
            "482915",
            UserRole::Admin,
            None,
        )
        .await
        .expect("create test admin");
        (admin.id, code)
    }

    async fn cleanup_test_admin(pool: &PgPool, code: &str) {
        let _ = sqlx::query("DELETE FROM employees WHERE employee_code = $1")
            .bind(code)
            .execute(pool)
            .await;
    }

    #[test]
    fn close_result_distinguishes_new_and_duplicate() {
        assert_ne!(
            ClosePayPeriodResult::Closed,
            ClosePayPeriodResult::AlreadyClosed
        );
    }

    #[tokio::test]
    async fn duplicate_close_returns_already_closed() {
        let Some(pool) = test_pool().await else {
            eprintln!("skipping payroll_controls test: DATABASE_URL not available");
            return;
        };

        let start = Date::from_calendar_date(2099, Month::January, 1).unwrap();
        let end = Date::from_calendar_date(2099, Month::January, 7).unwrap();
        let (admin_id, admin_code) = test_admin(&pool).await;

        let first = close_pay_period(&pool, start, end, admin_id, Some("unit test"))
            .await
            .expect("first close");
        assert_eq!(first, ClosePayPeriodResult::Closed);

        let second = close_pay_period(&pool, start, end, admin_id, Some("unit test"))
            .await
            .expect("second close");
        assert_eq!(second, ClosePayPeriodResult::AlreadyClosed);

        reopen_pay_period(&pool, start, end)
            .await
            .expect("cleanup reopen");
        cleanup_test_admin(&pool, &admin_code).await;
    }

    #[tokio::test]
    async fn rejects_overlapping_close_ranges() {
        let Some(pool) = test_pool().await else {
            eprintln!("skipping payroll_controls test: DATABASE_URL not available");
            return;
        };

        let (admin_id, admin_code) = test_admin(&pool).await;
        let first_start = Date::from_calendar_date(2099, Month::March, 1).unwrap();
        let first_end = Date::from_calendar_date(2099, Month::March, 7).unwrap();
        let overlap_start = Date::from_calendar_date(2099, Month::March, 5).unwrap();
        let overlap_end = Date::from_calendar_date(2099, Month::March, 10).unwrap();

        close_pay_period(&pool, first_start, first_end, admin_id, Some("first"))
            .await
            .expect("first close");

        let overlap =
            close_pay_period(&pool, overlap_start, overlap_end, admin_id, Some("overlap"))
                .await
                .expect_err("overlap close");
        assert!(
            matches!(overlap, AppError::BadRequest(msg) if msg.contains("overlaps existing closed period"))
        );

        reopen_pay_period(&pool, first_start, first_end)
            .await
            .expect("cleanup reopen");
        cleanup_test_admin(&pool, &admin_code).await;
    }

    async fn test_pool() -> Option<PgPool> {
        dotenvy::dotenv().ok();
        let url = std::env::var("DATABASE_URL").ok()?;
        let pool = crate::db::connect_with_options(&url, 1).await.ok()?;
        crate::db::migrate(&pool).await.ok()?;
        Some(pool)
    }
}
