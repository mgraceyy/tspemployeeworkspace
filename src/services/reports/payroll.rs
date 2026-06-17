use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{AttendanceStatus, OtStatus, UserRole};

#[derive(Debug, Clone, Default)]
pub struct PayrollFilters {
    pub department: Option<String>,
    pub role: Option<UserRole>,
    pub employee_id: Option<Uuid>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct PayrollRow {
    pub employee_code: String,
    pub full_name: String,
    pub department: Option<String>,
    pub regular_minutes: i64,
    pub approved_ot_minutes: i64,
    pub pending_ot_minutes: i64,
    pub sick_leave_days: i64,
    pub vacation_days: i64,
    pub official_leave_days: i64,
    pub offset_days: i64,
    pub no_show_days: i64,
}

#[derive(Debug, sqlx::FromRow)]
pub struct PayrollDetailRow {
    pub employee_code: String,
    pub full_name: String,
    pub department: Option<String>,
    pub work_date: Date,
    pub clock_in: Option<OffsetDateTime>,
    pub clock_out: Option<OffsetDateTime>,
    pub regular_minutes: Option<i32>,
    pub ot_minutes: i32,
    pub ot_status: OtStatus,
    pub attendance: Option<AttendanceStatus>,
}

pub async fn payroll_summary(
    pool: &PgPool,
    start: Date,
    end: Date,
    filters: &PayrollFilters,
) -> AppResult<Vec<PayrollRow>> {
    let department = filters
        .department
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty());
    let rows = sqlx::query_as::<_, PayrollRow>(
        "SELECT e.employee_code,
                e.full_name,
                p.department,
                COALESCE(SUM(te.regular_minutes), 0) AS regular_minutes,
                COALESCE(SUM(CASE WHEN te.ot_status = 'approved' THEN te.ot_minutes ELSE 0 END), 0) AS approved_ot_minutes,
                COALESCE(SUM(CASE WHEN te.ot_status = 'pending' THEN te.ot_minutes ELSE 0 END), 0) AS pending_ot_minutes,
                COALESCE(SUM(CASE WHEN te.attendance = 'sick_leave' THEN 1 ELSE 0 END), 0) AS sick_leave_days,
                COALESCE(SUM(CASE WHEN te.attendance = 'vacation' THEN 1 ELSE 0 END), 0) AS vacation_days,
                COALESCE(SUM(CASE WHEN te.attendance = 'official_leave' THEN 1 ELSE 0 END), 0) AS official_leave_days,
                COALESCE(SUM(CASE WHEN te.attendance = 'offset' THEN 1 ELSE 0 END), 0) AS offset_days,
                COALESCE(SUM(CASE WHEN te.attendance = 'no_show' THEN 1 ELSE 0 END), 0) AS no_show_days
         FROM employees e
         LEFT JOIN employee_profiles p ON p.employee_id = e.id
         LEFT JOIN time_entries te
           ON te.employee_id = e.id
          AND te.work_date BETWEEN $1 AND $2
         WHERE e.is_active = TRUE
           AND ($3::text IS NULL OR p.department = $3)
           AND ($4::user_role IS NULL OR e.role = $4)
           AND ($5::uuid IS NULL OR e.id = $5)
         GROUP BY e.id, e.employee_code, e.full_name, p.department
         ORDER BY e.full_name",
    )
    .bind(start)
    .bind(end)
    .bind(department)
    .bind(filters.role)
    .bind(filters.employee_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(rows)
}

pub async fn payroll_detail(
    pool: &PgPool,
    start: Date,
    end: Date,
    filters: &PayrollFilters,
) -> AppResult<Vec<PayrollDetailRow>> {
    let department = filters
        .department
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty());
    let rows = sqlx::query_as::<_, PayrollDetailRow>(
        "SELECT e.employee_code,
                e.full_name,
                p.department,
                te.work_date,
                te.clock_in,
                te.clock_out,
                te.regular_minutes,
                te.ot_minutes,
                te.ot_status,
                te.attendance
         FROM time_entries te
         JOIN employees e ON e.id = te.employee_id
         LEFT JOIN employee_profiles p ON p.employee_id = e.id
         WHERE te.work_date BETWEEN $1 AND $2
           AND e.is_active = TRUE
           AND ($3::text IS NULL OR p.department = $3)
           AND ($4::user_role IS NULL OR e.role = $4)
           AND ($5::uuid IS NULL OR e.id = $5)
         ORDER BY te.work_date, e.full_name",
    )
    .bind(start)
    .bind(end)
    .bind(department)
    .bind(filters.role)
    .bind(filters.employee_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub fn payable_minutes(row: &PayrollRow) -> i64 {
    row.regular_minutes + row.approved_ot_minutes
}

pub fn minutes_to_hours_decimal(minutes: i64) -> f64 {
    (minutes as f64) / 60.0
}

pub fn ot_status_payable(status: OtStatus) -> bool {
    status == OtStatus::Approved
}
