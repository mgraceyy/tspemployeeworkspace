use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{OtStatus, TimeEntry};
use crate::services::attendance::{evaluate_attendance, get_shift_for_date};
use crate::services::hours::calculate;
use crate::services::settings::get_settings;
use crate::services::team::assert_can_manage;

async fn apply_correction(
    pool: &PgPool,
    entry_id: Uuid,
    employee_id: Uuid,
    work_date: Date,
    editor_id: Uuid,
    old_clock_in: Option<OffsetDateTime>,
    old_clock_out: Option<OffsetDateTime>,
    new_clock_in: OffsetDateTime,
    new_clock_out: OffsetDateTime,
    reason: &str,
) -> AppResult<TimeEntry> {
    if new_clock_out <= new_clock_in {
        return Err(AppError::bad_request("Clock out must be after clock in"));
    }

    let settings = get_settings(pool).await?;
    let breakdown = calculate(new_clock_in, new_clock_out, &settings);
    let shift = get_shift_for_date(pool, employee_id, work_date).await?;
    let attendance =
        evaluate_attendance(new_clock_in, Some(new_clock_out), shift.as_ref(), &settings, work_date);

    let ot_status = if breakdown.ot_minutes > 0 {
        OtStatus::Pending
    } else {
        OtStatus::None
    };

    sqlx::query(
        "INSERT INTO correction_logs
            (time_entry_id, edited_by, reason, old_clock_in, old_clock_out, new_clock_in, new_clock_out)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(entry_id)
    .bind(editor_id)
    .bind(reason)
    .bind(old_clock_in)
    .bind(old_clock_out)
    .bind(new_clock_in)
    .bind(new_clock_out)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let updated = sqlx::query_as::<_, TimeEntry>(
        "UPDATE time_entries
         SET clock_in = $2,
             clock_out = $3,
             gross_minutes = $4,
             net_minutes = $5,
             regular_minutes = $6,
             ot_minutes = $7,
             ot_status = $8,
             ot_reviewed_by = NULL,
             ot_reviewed_at = NULL,
             ot_note = NULL,
             attendance = $9
         WHERE id = $1
         RETURNING id, employee_id, work_date, clock_in, clock_out,
                   gross_minutes, net_minutes, regular_minutes, ot_minutes,
                   ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, attendance, created_at",
    )
    .bind(entry_id)
    .bind(new_clock_in)
    .bind(new_clock_out)
    .bind(breakdown.gross_minutes)
    .bind(breakdown.net_minutes)
    .bind(breakdown.regular_minutes)
    .bind(breakdown.ot_minutes)
    .bind(ot_status)
    .bind(attendance)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(updated)
}

pub async fn correct_entry(
    pool: &PgPool,
    entry_id: Uuid,
    editor_id: Uuid,
    new_clock_in: OffsetDateTime,
    new_clock_out: OffsetDateTime,
    reason: &str,
    is_admin: bool,
    manager_id: Uuid,
) -> AppResult<TimeEntry> {
    let entry = sqlx::query_as::<_, TimeEntry>(
        "SELECT id, employee_id, work_date, clock_in, clock_out,
                gross_minutes, net_minutes, regular_minutes, ot_minutes,
                ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, attendance, created_at
         FROM time_entries
         WHERE id = $1",
    )
    .bind(entry_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::NotFound)?;

    assert_can_manage(pool, manager_id, entry.employee_id, is_admin).await?;

    apply_correction(
        pool,
        entry_id,
        entry.employee_id,
        entry.work_date,
        editor_id,
        entry.clock_in,
        entry.clock_out,
        new_clock_in,
        new_clock_out,
        reason,
    )
    .await
}

pub async fn create_corrected_entry(
    pool: &PgPool,
    employee_id: Uuid,
    work_date: Date,
    editor_id: Uuid,
    new_clock_in: OffsetDateTime,
    new_clock_out: OffsetDateTime,
    reason: &str,
    is_admin: bool,
    manager_id: Uuid,
) -> AppResult<TimeEntry> {
    assert_can_manage(pool, manager_id, employee_id, is_admin).await?;

    if let Some(existing) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM time_entries WHERE employee_id = $1 AND work_date = $2",
    )
    .bind(employee_id)
    .bind(work_date)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    {
        return correct_entry(
            pool,
            existing,
            editor_id,
            new_clock_in,
            new_clock_out,
            reason,
            is_admin,
            manager_id,
        )
        .await;
    }

    if new_clock_out <= new_clock_in {
        return Err(AppError::bad_request("Clock out must be after clock in"));
    }

    let settings = get_settings(pool).await?;
    let breakdown = calculate(new_clock_in, new_clock_out, &settings);
    let shift = get_shift_for_date(pool, employee_id, work_date).await?;
    let attendance =
        evaluate_attendance(new_clock_in, Some(new_clock_out), shift.as_ref(), &settings, work_date);

    let ot_status = if breakdown.ot_minutes > 0 {
        OtStatus::Pending
    } else {
        OtStatus::None
    };

    let entry = sqlx::query_as::<_, TimeEntry>(
        "INSERT INTO time_entries
            (employee_id, work_date, clock_in, clock_out, gross_minutes, net_minutes,
             regular_minutes, ot_minutes, ot_status, attendance)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING id, employee_id, work_date, clock_in, clock_out,
                   gross_minutes, net_minutes, regular_minutes, ot_minutes,
                   ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, attendance, created_at",
    )
    .bind(employee_id)
    .bind(work_date)
    .bind(new_clock_in)
    .bind(new_clock_out)
    .bind(breakdown.gross_minutes)
    .bind(breakdown.net_minutes)
    .bind(breakdown.regular_minutes)
    .bind(breakdown.ot_minutes)
    .bind(ot_status)
    .bind(attendance)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    sqlx::query(
        "INSERT INTO correction_logs
            (time_entry_id, edited_by, reason, old_clock_in, old_clock_out, new_clock_in, new_clock_out)
         VALUES ($1, $2, $3, NULL, NULL, $4, $5)",
    )
    .bind(entry.id)
    .bind(editor_id)
    .bind(reason)
    .bind(new_clock_in)
    .bind(new_clock_out)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(entry)
}