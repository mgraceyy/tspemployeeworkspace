use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{OtStatus, TimeEntryWithEmployee};

pub async fn list_pending_for_manager(
    pool: &PgPool,
    manager_id: Uuid,
    is_admin: bool,
) -> AppResult<Vec<TimeEntryWithEmployee>> {
    let entries = if is_admin {
        sqlx::query_as::<_, TimeEntryWithEmployee>(
            "SELECT te.id, te.employee_id, e.employee_code, e.full_name, te.work_date,
                    te.clock_in, te.clock_out, te.gross_minutes, te.net_minutes,
                    te.regular_minutes, te.ot_minutes, te.ot_status, te.ot_note, te.attendance
             FROM time_entries te
             JOIN employees e ON e.id = te.employee_id
             WHERE te.ot_status = 'pending'
             ORDER BY te.work_date DESC",
        )
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, TimeEntryWithEmployee>(
            "SELECT te.id, te.employee_id, e.employee_code, e.full_name, te.work_date,
                    te.clock_in, te.clock_out, te.gross_minutes, te.net_minutes,
                    te.regular_minutes, te.ot_minutes, te.ot_status, te.ot_note, te.attendance
             FROM time_entries te
             JOIN employees e ON e.id = te.employee_id
             WHERE te.ot_status = 'pending' AND e.manager_id = $1
             ORDER BY te.work_date DESC",
        )
        .bind(manager_id)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(entries)
}

pub async fn review_overtime(
    pool: &PgPool,
    entry_id: Uuid,
    reviewer_id: Uuid,
    approve: bool,
    note: Option<String>,
    is_admin: bool,
) -> AppResult<()> {
    let entry = sqlx::query_as::<_, (Uuid, Option<Uuid>, OtStatus)>(
        "SELECT te.id, e.manager_id, te.ot_status
         FROM time_entries te
         JOIN employees e ON e.id = te.employee_id
         WHERE te.id = $1",
    )
    .bind(entry_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::NotFound)?;

    if entry.2 != OtStatus::Pending {
        return Err(AppError::bad_request("Overtime is not pending approval"));
    }

    if !is_admin {
        let manager_id = entry.1;
        if manager_id != Some(reviewer_id) {
            return Err(AppError::Forbidden);
        }
    }

    let new_status = if approve {
        OtStatus::Approved
    } else {
        OtStatus::Rejected
    };

    sqlx::query(
        "UPDATE time_entries
         SET ot_status = $2,
             ot_reviewed_by = $3,
             ot_reviewed_at = now(),
             ot_note = $4
         WHERE id = $1",
    )
    .bind(entry_id)
    .bind(new_status)
    .bind(reviewer_id)
    .bind(note)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(())
}