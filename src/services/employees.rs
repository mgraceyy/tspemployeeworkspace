use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::pin::hash_pin;
use crate::error::{AppError, AppResult};
use crate::models::{Employee, EmployeeSummary, UserRole};

pub fn validate_pin(pin: &str) -> AppResult<()> {
    let len = pin.len();
    if (4..=6).contains(&len) && pin.chars().all(|c| c.is_ascii_digit()) {
        Ok(())
    } else {
        Err(AppError::bad_request("PIN must be 4–6 digits"))
    }
}

pub async fn find_by_code(pool: &PgPool, employee_code: &str) -> AppResult<Option<Employee>> {
    let employee = sqlx::query_as::<_, Employee>(
        "SELECT id, employee_code, full_name, pin_hash, role, manager_id, is_active,
                must_change_pin, created_at
         FROM employees
         WHERE employee_code = $1 AND is_active = TRUE",
    )
    .bind(employee_code)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(employee)
}

pub async fn find_by_id(pool: &PgPool, employee_id: Uuid) -> AppResult<Option<Employee>> {
    let employee = sqlx::query_as::<_, Employee>(
        "SELECT id, employee_code, full_name, pin_hash, role, manager_id, is_active,
                must_change_pin, created_at
         FROM employees
         WHERE id = $1",
    )
    .bind(employee_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(employee)
}

pub async fn list_all(pool: &PgPool) -> AppResult<Vec<EmployeeSummary>> {
    let employees = sqlx::query_as::<_, EmployeeSummary>(
        "SELECT id, employee_code, full_name, role, manager_id, is_active
         FROM employees
         ORDER BY full_name",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(employees)
}

pub async fn list_team(pool: &PgPool, manager_id: Uuid) -> AppResult<Vec<EmployeeSummary>> {
    let employees = sqlx::query_as::<_, EmployeeSummary>(
        "SELECT id, employee_code, full_name, role, manager_id, is_active
         FROM employees
         WHERE manager_id = $1 AND is_active = TRUE
         ORDER BY full_name",
    )
    .bind(manager_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(employees)
}

pub async fn create_employee(
    pool: &PgPool,
    employee_code: &str,
    full_name: &str,
    pin: &str,
    role: UserRole,
    manager_id: Option<Uuid>,
) -> AppResult<EmployeeSummary> {
    validate_pin(pin)?;
    let pin_hash = hash_pin(pin)?;
    let employee = sqlx::query_as::<_, EmployeeSummary>(
        "INSERT INTO employees (employee_code, full_name, pin_hash, role, manager_id, must_change_pin)
         VALUES ($1, $2, $3, $4, $5, TRUE)
         RETURNING id, employee_code, full_name, role, manager_id, is_active",
    )
    .bind(employee_code)
    .bind(full_name)
    .bind(pin_hash)
    .bind(role)
    .bind(manager_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(db_err) = &e {
            if db_err.constraint() == Some("employees_employee_code_key") {
                return AppError::bad_request("Employee code already exists");
            }
        }
        AppError::Internal(e.into())
    })?;
    Ok(employee)
}

pub async fn update_employee(
    pool: &PgPool,
    employee_id: Uuid,
    full_name: &str,
    role: UserRole,
    manager_id: Option<Uuid>,
) -> AppResult<EmployeeSummary> {
    if full_name.trim().is_empty() {
        return Err(AppError::bad_request("Full name is required"));
    }

    let employee = sqlx::query_as::<_, EmployeeSummary>(
        "UPDATE employees
         SET full_name = $2, role = $3, manager_id = $4
         WHERE id = $1
         RETURNING id, employee_code, full_name, role, manager_id, is_active",
    )
    .bind(employee_id)
    .bind(full_name.trim())
    .bind(role)
    .bind(manager_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::NotFound)?;

    Ok(employee)
}

pub async fn set_employee_active(
    pool: &PgPool,
    employee_id: Uuid,
    is_active: bool,
) -> AppResult<()> {
    let updated = sqlx::query(
        "UPDATE employees SET is_active = $2 WHERE id = $1",
    )
    .bind(employee_id)
    .bind(is_active)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn reset_employee_pin(
    pool: &PgPool,
    employee_id: Uuid,
    new_pin: &str,
) -> AppResult<()> {
    validate_pin(new_pin)?;
    let pin_hash = hash_pin(new_pin)?;
    let updated = sqlx::query(
        "UPDATE employees SET pin_hash = $2, must_change_pin = TRUE WHERE id = $1",
    )
    .bind(employee_id)
    .bind(pin_hash)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn change_own_pin(
    pool: &PgPool,
    employee_id: Uuid,
    new_pin: &str,
) -> AppResult<()> {
    validate_pin(new_pin)?;
    let pin_hash = hash_pin(new_pin)?;
    let updated = sqlx::query(
        "UPDATE employees SET pin_hash = $2, must_change_pin = FALSE WHERE id = $1",
    )
    .bind(employee_id)
    .bind(pin_hash)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn count_active_admins(pool: &PgPool) -> AppResult<i64> {
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM employees WHERE is_active = TRUE AND role = 'admin'",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(count.0)
}

pub async fn count_active(pool: &PgPool) -> AppResult<i64> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM employees WHERE is_active = TRUE")
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(count.0)
}

pub async fn seed_admin_if_empty(pool: &PgPool, seed_default_admin: bool) -> AppResult<()> {
    let count = count_active(pool).await?;
    if count > 0 {
        return Ok(());
    }

    if !seed_default_admin {
        tracing::warn!(
            "no employees found; set SEED_DEFAULT_ADMIN=true to create a default admin account"
        );
        return Ok(());
    }

    let pin_hash = hash_pin("1234")?;
    sqlx::query(
        "INSERT INTO employees (employee_code, full_name, pin_hash, role, must_change_pin)
         VALUES ('ADMIN', 'System Administrator', $1, 'admin', TRUE)",
    )
    .bind(pin_hash)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    tracing::warn!(
        "seeded default admin (ADMIN / 1234); change the PIN immediately after first login"
    );
    Ok(())
}