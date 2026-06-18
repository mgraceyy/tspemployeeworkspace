use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::pin::hash_pin;
use crate::error::{AppError, AppResult};
use crate::models::{PinResetRequest, PinResetRequestRow};
use crate::services::employees::{find_by_code, validate_pin};
use crate::services::team::assert_can_manage;

pub async fn create_request(
    pool: &PgPool,
    employee_id: Uuid,
    reason: Option<&str>,
) -> AppResult<PinResetRequest> {
    let reason = reason.map(str::trim).filter(|s| !s.is_empty());
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM pin_reset_requests
            WHERE employee_id = $1 AND status = 'pending'
        )",
    )
    .bind(employee_id)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if pending {
        return Err(AppError::bad_request(
            "You already have a pending PIN reset request",
        ));
    }

    sqlx::query_as::<_, PinResetRequest>(
        "INSERT INTO pin_reset_requests (employee_id, reason)
         VALUES ($1, $2)
         RETURNING id, employee_id, reason, status, requested_at, reviewed_by, reviewed_at, review_note",
    )
    .bind(employee_id)
    .bind(reason)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn create_request_by_code(
    pool: &PgPool,
    employee_code: &str,
    reason: Option<&str>,
) -> AppResult<()> {
    let code = employee_code.trim().to_uppercase();
    if code.is_empty() {
        return Ok(());
    }

    let Some(employee) = find_by_code(pool, &code).await? else {
        return Ok(());
    };

    let _ = create_request(pool, employee.id, reason).await;
    Ok(())
}

pub async fn get_pending_for_employee(
    pool: &PgPool,
    employee_id: Uuid,
) -> AppResult<Option<PinResetRequest>> {
    sqlx::query_as::<_, PinResetRequest>(
        "SELECT id, employee_id, reason, status, requested_at, reviewed_by, reviewed_at, review_note
         FROM pin_reset_requests
         WHERE employee_id = $1 AND status = 'pending'
         ORDER BY requested_at DESC
         LIMIT 1",
    )
    .bind(employee_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))
}

pub async fn list_pending_for_reviewer(
    pool: &PgPool,
    reviewer_id: Uuid,
    is_admin: bool,
) -> AppResult<Vec<PinResetRequestRow>> {
    let rows = if is_admin {
        sqlx::query_as::<_, PinResetRequestRow>(
            "SELECT r.id, r.employee_id, e.employee_code, e.full_name, r.reason, r.requested_at
             FROM pin_reset_requests r
             JOIN employees e ON e.id = r.employee_id
             WHERE r.status = 'pending'
             ORDER BY r.requested_at ASC",
        )
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, PinResetRequestRow>(
            "SELECT r.id, r.employee_id, e.employee_code, e.full_name, r.reason, r.requested_at
             FROM pin_reset_requests r
             JOIN employees e ON e.id = r.employee_id
             WHERE r.status = 'pending' AND e.manager_id = $1
             ORDER BY r.requested_at ASC",
        )
        .bind(reviewer_id)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn count_pending_for_reviewer(
    pool: &PgPool,
    reviewer_id: Uuid,
    is_admin: bool,
) -> AppResult<i64> {
    let count: i64 = if is_admin {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM pin_reset_requests WHERE status = 'pending'",
        )
        .fetch_one(pool)
        .await
    } else {
        sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM pin_reset_requests r
             JOIN employees e ON e.id = r.employee_id
             WHERE r.status = 'pending' AND e.manager_id = $1",
        )
        .bind(reviewer_id)
        .fetch_one(pool)
        .await
    }
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(count)
}

async fn load_pending_request(pool: &PgPool, request_id: Uuid) -> AppResult<PinResetRequest> {
    sqlx::query_as::<_, PinResetRequest>(
        "SELECT id, employee_id, reason, status, requested_at, reviewed_by, reviewed_at, review_note
         FROM pin_reset_requests
         WHERE id = $1 AND status = 'pending'",
    )
    .bind(request_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::NotFound)
}

pub async fn approve_request(
    pool: &PgPool,
    request_id: Uuid,
    reviewer_id: Uuid,
    is_admin: bool,
    temp_pin: &str,
) -> AppResult<()> {
    let request = load_pending_request(pool, request_id).await?;
    assert_can_manage(pool, reviewer_id, request.employee_id, is_admin).await?;
    validate_pin(temp_pin)?;
    let pin_hash = hash_pin(temp_pin)?;

    let mut tx = pool.begin().await.map_err(|e| AppError::Internal(e.into()))?;

    sqlx::query(
        "UPDATE employees
         SET pin_hash = $2, must_change_pin = TRUE, session_version = session_version + 1
         WHERE id = $1",
    )
    .bind(request.employee_id)
    .bind(pin_hash)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    sqlx::query(
        "UPDATE pin_reset_requests
         SET status = 'approved', reviewed_by = $2, reviewed_at = now()
         WHERE id = $1",
    )
    .bind(request_id)
    .bind(reviewer_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    tx.commit().await.map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

pub async fn deny_request(
    pool: &PgPool,
    request_id: Uuid,
    reviewer_id: Uuid,
    is_admin: bool,
    note: &str,
) -> AppResult<()> {
    let note = note.trim();
    if note.is_empty() {
        return Err(AppError::bad_request("A denial reason is required"));
    }

    let request = load_pending_request(pool, request_id).await?;
    assert_can_manage(pool, reviewer_id, request.employee_id, is_admin).await?;

    sqlx::query(
        "UPDATE pin_reset_requests
         SET status = 'denied', reviewed_by = $2, reviewed_at = now(), review_note = $3
         WHERE id = $1",
    )
    .bind(request_id)
    .bind(reviewer_id)
    .bind(note)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

pub async fn cancel_own_request(pool: &PgPool, employee_id: Uuid, request_id: Uuid) -> AppResult<()> {
    let updated = sqlx::query(
        "UPDATE pin_reset_requests
         SET status = 'cancelled', reviewed_at = now()
         WHERE id = $1 AND employee_id = $2 AND status = 'pending'",
    )
    .bind(request_id)
    .bind(employee_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}