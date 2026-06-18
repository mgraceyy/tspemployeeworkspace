use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::pin::hash_pin;
use crate::error::{AppError, AppResult};
use crate::models::{Employee, EmployeeSummary, UserRole};

const WEAK_PINS: &[&str] = &[
    "0000", "1111", "2222", "3333", "4444", "5555", "6666", "7777", "8888", "9999", "1234", "4321",
    "1212", "6969", "1004", "2000", "12345", "11111", "123456", "654321", "000000", "123123",
];

pub fn validate_pin(pin: &str) -> AppResult<()> {
    let len = pin.len();
    if !(4..=6).contains(&len) || !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::bad_request("PIN must be 4–6 digits"));
    }
    if WEAK_PINS.contains(&pin) {
        return Err(AppError::bad_request(
            "This PIN is too easy to guess — choose a different one",
        ));
    }
    if pin.chars().all(|c| c == pin.chars().next().unwrap()) {
        return Err(AppError::bad_request(
            "PIN cannot be the same digit repeated — choose a different one",
        ));
    }
    Ok(())
}

pub async fn find_by_code(pool: &PgPool, employee_code: &str) -> AppResult<Option<Employee>> {
    let employee = sqlx::query_as::<_, Employee>(
        "SELECT id, employee_code, full_name, pin_hash, role, manager_id, is_active,
                must_change_pin, session_version, created_at
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
                must_change_pin, session_version, created_at
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

    crate::services::profile::ensure_profile(pool, employee.id).await?;
    crate::services::requirements::seed_for_employee(pool, employee.id).await?;

    Ok(employee)
}

pub async fn update_employee(
    pool: &PgPool,
    employee_id: Uuid,
    employee_code: &str,
    full_name: &str,
    role: UserRole,
    manager_id: Option<Uuid>,
) -> AppResult<EmployeeSummary> {
    let employee_code = employee_code.trim().to_uppercase();
    if employee_code.is_empty() {
        return Err(AppError::bad_request("Employee code is required"));
    }
    if full_name.trim().is_empty() {
        return Err(AppError::bad_request("Full name is required"));
    }

    let employee = sqlx::query_as::<_, EmployeeSummary>(
        "UPDATE employees
         SET employee_code = $2, full_name = $3, role = $4, manager_id = $5
         WHERE id = $1
         RETURNING id, employee_code, full_name, role, manager_id, is_active",
    )
    .bind(employee_id)
    .bind(&employee_code)
    .bind(full_name.trim())
    .bind(role)
    .bind(manager_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(db_err) = &e {
            if db_err.constraint() == Some("employees_employee_code_key") {
                return AppError::bad_request("Employee code already exists");
            }
        }
        AppError::Internal(e.into())
    })?
    .ok_or(AppError::NotFound)?;

    Ok(employee)
}

pub async fn set_employee_active(
    pool: &PgPool,
    employee_id: Uuid,
    is_active: bool,
) -> AppResult<()> {
    let updated = sqlx::query("UPDATE employees SET is_active = $2 WHERE id = $1")
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

pub async fn reset_employee_pin(pool: &PgPool, employee_id: Uuid, new_pin: &str) -> AppResult<()> {
    validate_pin(new_pin)?;
    let pin_hash = hash_pin(new_pin)?;
    let updated = sqlx::query(
        "UPDATE employees
         SET pin_hash = $2, must_change_pin = TRUE, session_version = session_version + 1
         WHERE id = $1",
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

pub async fn bump_session_version(pool: &PgPool, employee_id: Uuid) -> AppResult<i32> {
    let version: i32 = sqlx::query_scalar(
        "UPDATE employees
         SET session_version = session_version + 1
         WHERE id = $1
         RETURNING session_version",
    )
    .bind(employee_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::NotFound)?;
    Ok(version)
}

pub async fn change_own_pin(pool: &PgPool, employee_id: Uuid, new_pin: &str) -> AppResult<()> {
    validate_pin(new_pin)?;
    let pin_hash = hash_pin(new_pin)?;
    let updated =
        sqlx::query("UPDATE employees SET pin_hash = $2, must_change_pin = FALSE WHERE id = $1")
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
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM employees WHERE is_active = TRUE AND role = 'admin'")
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

pub async fn seed_e2e_fixtures(pool: &PgPool, enabled: bool) -> AppResult<()> {
    use crate::services::requirements::create_type;

    if !enabled {
        return Ok(());
    }

    let manager_id = if let Some(manager) = find_by_code(pool, "E2MGR").await? {
        manager.id
    } else {
        let manager = create_employee(
            pool,
            "E2MGR",
            "E2E Manager",
            "482915",
            UserRole::Manager,
            None,
        )
        .await?;
        tracing::info!("seeded E2E fixture manager (E2MGR / 482915)");
        manager.id
    };

    if find_by_code(pool, "E2E001").await?.is_none() {
        create_employee(
            pool,
            "E2E001",
            "E2E Test Employee",
            "482915",
            UserRole::Employee,
            Some(manager_id),
        )
        .await?;
        tracing::info!("seeded E2E fixture employee (E2E001 / 482915)");
    }

    sqlx::query(
        "UPDATE employees SET must_change_pin = FALSE
         WHERE employee_code IN ('E2MGR', 'E2E001')",
    )
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if let Ok(settings) = crate::services::settings::get_settings(pool).await {
        if let Some(admin) = find_by_code(pool, "ADMIN").await? {
            let effective =
                crate::services::timezone::company_date_now(&settings)? - time::Duration::days(365);
            let missing_ids: Vec<uuid::Uuid> = sqlx::query_scalar(
                "SELECT e.id FROM employees e
                 LEFT JOIN compensation_profiles c ON c.employee_id = e.id
                 WHERE e.is_active = TRUE AND c.employee_id IS NULL",
            )
            .fetch_all(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
            let seeded_count = missing_ids.len();
            for employee_id in missing_ids {
                crate::services::compensation::upsert_profile(
                    pool,
                    employee_id,
                    2_500_000,
                    132,
                    0,
                    0,
                    effective,
                    admin.id,
                )
                .await?;
            }
            if seeded_count > 0 {
                tracing::info!("seeded E2E compensation for {seeded_count} active employee(s)");
            }
        }
    }

    let doc_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM requirement_types WHERE name = 'E2E Test Document')",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if !doc_exists {
        create_type(
            pool,
            "E2E Test Document",
            "Playwright upload smoke test",
            true,
            false,
            1,
            None,
        )
        .await?;
        tracing::info!("seeded E2E requirement type (E2E Test Document)");
    }

    if let (Some(employee), Ok(settings)) = (
        find_by_code(pool, "E2E001").await?,
        crate::services::settings::get_settings(pool).await,
    ) {
        let today = crate::services::timezone::company_date_now(&settings)?;
        let pending_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM time_entries
                WHERE employee_id = $1 AND work_date = $2 AND ot_status = 'pending'
            )",
        )
        .bind(employee.id)
        .bind(today)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if !pending_exists {
            sqlx::query(
                "INSERT INTO time_entries
                    (employee_id, work_date, regular_minutes, ot_minutes, ot_status, attendance, ot_reason)
                 VALUES ($1, $2, 480, 45, 'pending', 'on_time', 'E2E overtime')",
            )
            .bind(employee.id)
            .bind(today)
            .execute(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
            tracing::info!("seeded E2E pending OT entry for E2E001");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_weak_pins() {
        assert!(validate_pin("1234").is_err());
        assert!(validate_pin("1111").is_err());
        assert!(validate_pin("12345").is_err());
    }

    #[test]
    fn accepts_reasonable_pins() {
        assert!(validate_pin("482915").is_ok());
        assert!(validate_pin("7391").is_ok());
    }
}
