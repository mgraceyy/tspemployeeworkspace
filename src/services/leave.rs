use sqlx::{PgPool, Postgres, Transaction};
use time::Date;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{
    AttendanceStatus, LeaveDayPortion, LeaveRequest, LeaveRequestStatus, LeaveRequestType,
    LeaveRequestWithEmployee,
};

use crate::services::attendance::{
    clear_leave_absence_for_employee_in_tx, mark_absence_for_employee_in_tx,
};
use crate::services::leave_balances::{
    assert_sufficient_balance, deduct_for_approved_request_in_tx, refund_for_approved_request_in_tx,
};
use crate::services::payroll_controls::assert_date_range_editable;
use crate::services::team::assert_can_manage;

pub fn leave_type_to_attendance(leave_type: LeaveRequestType) -> AttendanceStatus {
    match leave_type {
        LeaveRequestType::SickLeave => AttendanceStatus::SickLeave,
        LeaveRequestType::Vacation => AttendanceStatus::Vacation,
        LeaveRequestType::OfficialLeave => AttendanceStatus::OfficialLeave,
        LeaveRequestType::Offset => AttendanceStatus::Offset,
    }
}

pub async fn list_for_employee(pool: &PgPool, employee_id: Uuid) -> AppResult<Vec<LeaveRequest>> {
    let rows = sqlx::query_as::<_, LeaveRequest>(
        "SELECT id, employee_id, start_date, end_date, day_portion, leave_type, reason, status,
                reviewer_note, reviewed_at, created_at
         FROM leave_requests
         WHERE employee_id = $1
         ORDER BY created_at DESC",
    )
    .bind(employee_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn list_pending_for_manager(
    pool: &PgPool,
    manager_id: Uuid,
    is_admin: bool,
) -> AppResult<Vec<LeaveRequestWithEmployee>> {
    let rows = if is_admin {
        sqlx::query_as::<_, LeaveRequestWithEmployee>(
            "SELECT lr.id, lr.employee_id, e.employee_code, e.full_name,
                    lr.start_date, lr.end_date, lr.day_portion, lr.leave_type, lr.reason, lr.status, lr.created_at
             FROM leave_requests lr
             JOIN employees e ON e.id = lr.employee_id
             WHERE lr.status = 'pending'
             ORDER BY lr.created_at",
        )
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, LeaveRequestWithEmployee>(
            "SELECT lr.id, lr.employee_id, e.employee_code, e.full_name,
                    lr.start_date, lr.end_date, lr.day_portion, lr.leave_type, lr.reason, lr.status, lr.created_at
             FROM leave_requests lr
             JOIN employees e ON e.id = lr.employee_id
             WHERE lr.status = 'pending' AND e.manager_id = $1
             ORDER BY lr.created_at",
        )
        .bind(manager_id)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn count_pending_for_manager(
    pool: &PgPool,
    manager_id: Uuid,
    is_admin: bool,
) -> AppResult<i64> {
    let count: i64 = if is_admin {
        sqlx::query_scalar("SELECT COUNT(*) FROM leave_requests WHERE status = 'pending'")
            .fetch_one(pool)
            .await
    } else {
        sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM leave_requests lr
             JOIN employees e ON e.id = lr.employee_id
             WHERE lr.status = 'pending' AND e.manager_id = $1",
        )
        .bind(manager_id)
        .fetch_one(pool)
        .await
    }
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(count)
}

pub async fn list_approved_for_manager(
    pool: &PgPool,
    manager_id: Uuid,
    is_admin: bool,
) -> AppResult<Vec<LeaveRequestWithEmployee>> {
    let rows = if is_admin {
        sqlx::query_as::<_, LeaveRequestWithEmployee>(
            "SELECT lr.id, lr.employee_id, e.employee_code, e.full_name,
                    lr.start_date, lr.end_date, lr.day_portion, lr.leave_type, lr.reason, lr.status, lr.created_at
             FROM leave_requests lr
             JOIN employees e ON e.id = lr.employee_id
             WHERE lr.status = 'approved'
             ORDER BY lr.start_date DESC, lr.created_at DESC
             LIMIT 50",
        )
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, LeaveRequestWithEmployee>(
            "SELECT lr.id, lr.employee_id, e.employee_code, e.full_name,
                    lr.start_date, lr.end_date, lr.day_portion, lr.leave_type, lr.reason, lr.status, lr.created_at
             FROM leave_requests lr
             JOIN employees e ON e.id = lr.employee_id
             WHERE lr.status = 'approved' AND e.manager_id = $1
             ORDER BY lr.start_date DESC, lr.created_at DESC
             LIMIT 50",
        )
        .bind(manager_id)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

fn map_leave_request_conflict(error: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(db_err) = &error {
        if db_err.constraint() == Some("leave_requests_no_overlap_pending_approved") {
            return AppError::bad_request(
                "You already have a pending or approved leave request for overlapping dates",
            );
        }
    }
    AppError::Internal(error.into())
}

async fn has_overlapping_request(
    pool: &PgPool,
    employee_id: Uuid,
    start_date: Date,
    end_date: Date,
    exclude_id: Option<Uuid>,
) -> AppResult<bool> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM leave_requests
            WHERE employee_id = $1
              AND status IN ('pending', 'approved')
              AND ($4::uuid IS NULL OR id <> $4)
              AND start_date <= $3
              AND end_date >= $2
         )",
    )
    .bind(employee_id)
    .bind(start_date)
    .bind(end_date)
    .bind(exclude_id)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(exists)
}

pub async fn create_request(
    pool: &PgPool,
    employee_id: Uuid,
    start_date: Date,
    end_date: Date,
    day_portion: LeaveDayPortion,
    leave_type: LeaveRequestType,
    reason: Option<&str>,
) -> AppResult<LeaveRequest> {
    let reason = reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::bad_request("A reason is required for leave requests"))?;

    if end_date < start_date {
        return Err(AppError::bad_request(
            "End date must be on or after start date",
        ));
    }

    if has_overlapping_request(pool, employee_id, start_date, end_date, None).await? {
        return Err(AppError::bad_request(
            "You already have a pending or approved leave request for overlapping dates",
        ));
    }

    assert_date_range_editable(pool, start_date, end_date).await?;
    assert_sufficient_balance(
        pool,
        employee_id,
        leave_type,
        start_date,
        end_date,
        day_portion,
    )
    .await?;

    let row = sqlx::query_as::<_, LeaveRequest>(
        "INSERT INTO leave_requests (employee_id, start_date, end_date, day_portion, leave_type, reason)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, employee_id, start_date, end_date, day_portion, leave_type, reason, status,
                   reviewer_note, reviewed_at, created_at",
    )
    .bind(employee_id)
    .bind(start_date)
    .bind(end_date)
    .bind(day_portion)
    .bind(leave_type)
    .bind(reason)
    .fetch_one(pool)
    .await
    .map_err(map_leave_request_conflict)?;
    Ok(row)
}

pub async fn cancel_request(pool: &PgPool, employee_id: Uuid, request_id: Uuid) -> AppResult<()> {
    let updated = sqlx::query(
        "UPDATE leave_requests
         SET status = 'cancelled'
         WHERE id = $1 AND employee_id = $2 AND status = 'pending'",
    )
    .bind(request_id)
    .bind(employee_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if updated.rows_affected() > 0 {
        return Ok(());
    }

    revoke_approved_request(pool, request_id, employee_id, employee_id, false).await?;
    Ok(())
}

async fn revoke_approved_leave_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    request: &LeaveRequestWithEmployee,
    reviewer_id: Uuid,
) -> AppResult<()> {
    refund_for_approved_request_in_tx(
        tx,
        request.employee_id,
        request.leave_type,
        request.start_date,
        request.end_date,
        request.day_portion,
    )
    .await?;
    let attendance = leave_type_to_attendance(request.leave_type);
    let mut day = request.start_date;
    while day <= request.end_date {
        clear_leave_absence_for_employee_in_tx(tx, request.employee_id, day, attendance).await?;
        day += time::Duration::days(1);
    }

    let updated = sqlx::query(
        "UPDATE leave_requests
         SET status = 'cancelled', reviewer_id = $2, reviewed_at = now()
         WHERE id = $1 AND status = 'approved'",
    )
    .bind(request.id)
    .bind(reviewer_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if updated.rows_affected() == 0 {
        return Err(AppError::bad_request("Leave request is not approved"));
    }
    Ok(())
}

pub async fn revoke_approved_request(
    pool: &PgPool,
    request_id: Uuid,
    actor_id: Uuid,
    employee_id: Uuid,
    is_admin: bool,
) -> AppResult<LeaveRequestWithEmployee> {
    if actor_id != employee_id {
        assert_can_manage(pool, actor_id, employee_id, is_admin).await?;
    }

    let dates: (Date, Date) = sqlx::query_as(
        "SELECT start_date, end_date FROM leave_requests
         WHERE id = $1 AND employee_id = $2 AND status = 'approved'",
    )
    .bind(request_id)
    .bind(employee_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::bad_request(
        "Only approved leave in an open pay period can be revoked",
    ))?;
    assert_date_range_editable(pool, dates.0, dates.1).await?;

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let request = sqlx::query_as::<_, LeaveRequestWithEmployee>(
        "SELECT lr.id, lr.employee_id, e.employee_code, e.full_name,
                lr.start_date, lr.end_date, lr.day_portion, lr.leave_type, lr.reason, lr.status, lr.created_at
         FROM leave_requests lr
         JOIN employees e ON e.id = lr.employee_id
         WHERE lr.id = $1 AND lr.employee_id = $2 AND lr.status = 'approved'
         FOR UPDATE OF lr",
    )
    .bind(request_id)
    .bind(employee_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::bad_request(
        "Only approved leave in an open pay period can be revoked",
    ))?;

    revoke_approved_leave_in_tx(&mut tx, &request, actor_id).await?;

    tx.commit()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(request)
}

pub async fn revoke_approved_request_by_id(
    pool: &PgPool,
    request_id: Uuid,
    actor_id: Uuid,
    is_admin: bool,
) -> AppResult<LeaveRequestWithEmployee> {
    let employee_id: Uuid = sqlx::query_scalar(
        "SELECT employee_id FROM leave_requests WHERE id = $1 AND status = 'approved'",
    )
    .bind(request_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::bad_request(
        "Only approved leave in an open pay period can be revoked",
    ))?;

    revoke_approved_request(pool, request_id, actor_id, employee_id, is_admin).await
}

pub async fn review_request(
    pool: &PgPool,
    request_id: Uuid,
    reviewer_id: Uuid,
    is_admin: bool,
    approve: bool,
    note: Option<&str>,
) -> AppResult<LeaveRequestWithEmployee> {
    let employee_id: Uuid = sqlx::query_scalar(
        "SELECT employee_id FROM leave_requests WHERE id = $1 AND status = 'pending'",
    )
    .bind(request_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::bad_request("Leave request is not pending"))?;

    assert_can_manage(pool, reviewer_id, employee_id, is_admin).await?;

    if approve {
        let dates: (Date, Date) = sqlx::query_as(
            "SELECT start_date, end_date FROM leave_requests WHERE id = $1 AND status = 'pending'",
        )
        .bind(request_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        assert_date_range_editable(pool, dates.0, dates.1).await?;
    }

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let request = sqlx::query_as::<_, LeaveRequestWithEmployee>(
        "SELECT lr.id, lr.employee_id, e.employee_code, e.full_name,
                lr.start_date, lr.end_date, lr.day_portion, lr.leave_type, lr.reason, lr.status, lr.created_at
         FROM leave_requests lr
         JOIN employees e ON e.id = lr.employee_id
         WHERE lr.id = $1 AND lr.status = 'pending'
         FOR UPDATE OF lr",
    )
    .bind(request_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::bad_request("Leave request is not pending"))?;

    if approve {
        deduct_for_approved_request_in_tx(
            &mut tx,
            request.employee_id,
            request.leave_type,
            request.start_date,
            request.end_date,
            request.day_portion,
        )
        .await?;
        let attendance = leave_type_to_attendance(request.leave_type);
        let mut day = request.start_date;
        while day <= request.end_date {
            mark_absence_for_employee_in_tx(
                &mut tx,
                request.employee_id,
                day,
                attendance,
                reviewer_id,
            )
            .await?;
            day += time::Duration::days(1);
        }
    }

    let status = if approve {
        LeaveRequestStatus::Approved
    } else {
        LeaveRequestStatus::Rejected
    };

    let updated = sqlx::query(
        "UPDATE leave_requests
         SET status = $2, reviewer_id = $3, reviewer_note = $4, reviewed_at = now()
         WHERE id = $1 AND status = 'pending'",
    )
    .bind(request_id)
    .bind(status)
    .bind(reviewer_id)
    .bind(note.map(str::trim).filter(|value| !value.is_empty()))
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if updated.rows_affected() == 0 {
        return Err(AppError::bad_request("Leave request is not pending"));
    }

    tx.commit()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    Ok(request)
}
