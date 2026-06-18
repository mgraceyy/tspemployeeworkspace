use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::services::profile::ensure_profile;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AdminEmployeeRow {
    pub id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub role: crate::models::UserRole,
    pub is_active: bool,
    pub department: Option<String>,
    pub job_title: Option<String>,
    pub date_hired: Option<Date>,
    pub employment_type: Option<String>,
    pub contact_number: Option<String>,
    pub personal_email: Option<String>,
    pub work_location: Option<String>,
    pub requirements_met: i64,
    pub requirements_total: i64,
}

pub fn profile_completeness_pct(row: &AdminEmployeeRow) -> i32 {
    let filled = [
        row.department
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty()),
        row.job_title
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty()),
        row.date_hired.is_some(),
        row.employment_type
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty()),
        row.contact_number
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty()),
        row.personal_email
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty()),
        row.work_location
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty()),
    ]
    .iter()
    .filter(|&&set| set)
    .count();
    ((filled as f64 / 7.0) * 100.0).round() as i32
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EmployeeListStatus {
    #[default]
    Active,
    Archived,
    All,
}

impl EmployeeListStatus {
    pub fn from_query(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("archived") => Self::Archived,
            Some("all") => Self::All,
            _ => Self::Active,
        }
    }

    fn sql_clause(&self) -> &'static str {
        match self {
            Self::Active => "e.is_active = TRUE",
            Self::Archived => "e.is_active = FALSE",
            Self::All => "TRUE",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AdminEmployeeQuery {
    pub search: Option<String>,
    pub status: EmployeeListStatus,
    pub limit: i64,
    pub offset: i64,
}

fn employee_search_clause() -> &'static str {
    "($1::text IS NULL OR (
         e.employee_code ILIKE $1
         OR e.full_name ILIKE $1
         OR COALESCE(p.department, '') ILIKE $1
     ))"
}

pub async fn count_admin_employee_rows(
    pool: &PgPool,
    search: Option<&str>,
    status: EmployeeListStatus,
) -> AppResult<i64> {
    let pattern = crate::services::pagination::search_pattern(search);
    let count: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*)
         FROM employees e
         LEFT JOIN employee_profiles p ON p.employee_id = e.id
         WHERE {} AND {}",
        employee_search_clause(),
        status.sql_clause()
    ))
    .bind(pattern)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(count)
}

pub async fn list_admin_employee_rows(
    pool: &PgPool,
    query: &AdminEmployeeQuery,
) -> AppResult<Vec<AdminEmployeeRow>> {
    let pattern = crate::services::pagination::search_pattern(query.search.as_deref());
    let rows = sqlx::query_as::<_, AdminEmployeeRow>(&format!(
        "SELECT e.id, e.employee_code, e.full_name, e.role, e.is_active,
                p.department, p.job_title, p.date_hired, p.employment_type,
                p.contact_number, p.personal_email, p.work_location,
                COALESCE(r.approved_count, 0) AS requirements_met,
                COALESCE(r.total_count, 0) AS requirements_total
         FROM employees e
         LEFT JOIN employee_profiles p ON p.employee_id = e.id
         LEFT JOIN (
             SELECT er.employee_id,
                    COUNT(*) FILTER (
                        WHERE er.status = 'approved'
                          AND (er.expires_at IS NULL OR er.expires_at > now())
                    ) AS approved_count,
                    COUNT(*) AS total_count
             FROM employee_requirements er
             JOIN requirement_types rt ON rt.id = er.requirement_type_id AND rt.is_active = TRUE
             GROUP BY er.employee_id
         ) r ON r.employee_id = e.id
         WHERE {} AND {}
         ORDER BY e.full_name
         LIMIT $2 OFFSET $3",
        employee_search_clause(),
        query.status.sql_clause()
    ))
    .bind(pattern)
    .bind(query.limit)
    .bind(query.offset)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn count_active_without_department(pool: &PgPool) -> AppResult<i64> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM employees e
         LEFT JOIN employee_profiles p ON p.employee_id = e.id
         WHERE e.is_active = TRUE
           AND e.role = 'employee'
           AND (p.department IS NULL OR trim(p.department) = '')",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(count)
}

pub async fn list_distinct_departments(pool: &PgPool) -> AppResult<Vec<String>> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT department
         FROM employee_profiles
         WHERE department IS NOT NULL AND trim(department) <> ''
         ORDER BY department",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn bulk_assign_department(
    pool: &PgPool,
    employee_ids: &[Uuid],
    department: &str,
    editor_id: Uuid,
) -> AppResult<usize> {
    let dept = department.trim();
    if dept.is_empty() {
        return Err(AppError::bad_request("Department is required"));
    }
    if employee_ids.is_empty() {
        return Err(AppError::bad_request("Select at least one employee"));
    }

    for employee_id in employee_ids {
        ensure_profile(pool, *employee_id).await?;
    }

    let updated = sqlx::query(
        "UPDATE employee_profiles
         SET department = $2, updated_at = now(), updated_by = $3
         WHERE employee_id = ANY($1)",
    )
    .bind(employee_ids)
    .bind(dept)
    .bind(editor_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(updated.rows_affected() as usize)
}
