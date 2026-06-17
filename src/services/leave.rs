use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{
    AttendanceStatus, LeaveRequest, LeaveRequestStatus, LeaveRequestType, LeaveRequestWithEmployee,
};
use crate::services::attendance::mark_absence_for_employee;
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
        "SELECT id, employee_id, start_date, end_date, leave_type, reason, status,
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
                    lr.start_date, lr.end_date, lr.leave_type, lr.reason, lr.status, lr.created_at
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
                    lr.start_date, lr.end_date, lr.leave_type, lr.reason, lr.status, lr.created_at
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

    let row = sqlx::query_as::<_, LeaveRequest>(
        "INSERT INTO leave_requests (employee_id, start_date, end_date, leave_type, reason)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, employee_id, start_date, end_date, leave_type, reason, status,
                   reviewer_note, reviewed_at, created_at",
    )
    .bind(employee_id)
    .bind(start_date)
    .bind(end_date)
    .bind(leave_type)
    .bind(reason)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
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

    if updated.rows_affected() == 0 {
        return Err(AppError::bad_request(
            "Only pending leave requests can be cancelled",
        ));
    }
    Ok(())
}

pub async fn review_request(
    pool: &PgPool,
    request_id: Uuid,
    reviewer_id: Uuid,
    is_admin: bool,
    approve: bool,
    note: Option<&str>,
) -> AppResult<LeaveRequestWithEmployee> {
    let request = sqlx::query_as::<_, LeaveRequestWithEmployee>(
        "SELECT lr.id, lr.employee_id, e.employee_code, e.full_name,
                lr.start_date, lr.end_date, lr.leave_type, lr.reason, lr.status, lr.created_at
         FROM leave_requests lr
         JOIN employees e ON e.id = lr.employee_id
         WHERE lr.id = $1 AND lr.status = 'pending'",
    )
    .bind(request_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::bad_request("Leave request is not pending"))?;

    assert_can_manage(pool, reviewer_id, request.employee_id, is_admin).await?;

    if approve {
        assert_date_range_editable(pool, request.start_date, request.end_date).await?;
        let attendance = leave_type_to_attendance(request.leave_type);
        let mut day = request.start_date;
        while day <= request.end_date {
            mark_absence_for_employee(
                pool,
                request.employee_id,
                day,
                attendance,
                reviewer_id,
                is_admin,
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

    sqlx::query(
        "UPDATE leave_requests
         SET status = $2, reviewer_id = $3, reviewer_note = $4, reviewed_at = now()
         WHERE id = $1",
    )
    .bind(request_id)
    .bind(status)
    .bind(reviewer_id)
    .bind(note.map(str::trim).filter(|value| !value.is_empty()))
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(request)
}
