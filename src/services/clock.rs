use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{AttendanceStatus, OtStatus, TimeEntry};
use crate::services::attendance::{evaluate_attendance, get_shift_for_date};
use crate::services::hours::calculate;
use crate::services::settings::get_settings;
use crate::services::timezone::{manila_date_now, now_manila};

pub async fn get_today_entry(pool: &PgPool, employee_id: Uuid) -> AppResult<Option<TimeEntry>> {
    let today = manila_date_now();
    get_entry_for_date(pool, employee_id, today).await
}

pub async fn get_entry_for_date(
    pool: &PgPool,
    employee_id: Uuid,
    work_date: Date,
) -> AppResult<Option<TimeEntry>> {
    let entry = sqlx::query_as::<_, TimeEntry>(
        "SELECT id, employee_id, work_date, clock_in, clock_out,
                gross_minutes, net_minutes, regular_minutes, ot_minutes,
                ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, attendance, created_at
         FROM time_entries
         WHERE employee_id = $1 AND work_date = $2",
    )
    .bind(employee_id)
    .bind(work_date)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(entry)
}

pub async fn clock_in(pool: &PgPool, employee_id: Uuid) -> AppResult<TimeEntry> {
    let today = manila_date_now();
    let now = now_manila();

    if let Some(existing) = get_entry_for_date(pool, employee_id, today).await? {
        if existing.clock_in.is_some() && existing.clock_out.is_none() {
            return Err(AppError::bad_request("Already clocked in"));
        }
        if existing.clock_out.is_some() {
            return Err(AppError::bad_request("Already completed today"));
        }
    }

    let settings = get_settings(pool).await?;
    let shift = get_shift_for_date(pool, employee_id, today).await?;
    let attendance = evaluate_attendance(now, None, shift.as_ref(), &settings, today);

    let entry = sqlx::query_as::<_, TimeEntry>(
        "INSERT INTO time_entries (employee_id, work_date, clock_in, attendance)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (employee_id, work_date)
         DO UPDATE SET clock_in = EXCLUDED.clock_in, attendance = EXCLUDED.attendance
         RETURNING id, employee_id, work_date, clock_in, clock_out,
                   gross_minutes, net_minutes, regular_minutes, ot_minutes,
                   ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, attendance, created_at",
    )
    .bind(employee_id)
    .bind(today)
    .bind(now)
    .bind(attendance)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(entry)
}

pub async fn clock_out(pool: &PgPool, employee_id: Uuid) -> AppResult<TimeEntry> {
    let today = manila_date_now();
    let now = now_manila();
    let settings = get_settings(pool).await?;

    let entry = get_entry_for_date(pool, employee_id, today)
        .await?
        .ok_or_else(|| AppError::bad_request("No clock-in found for today"))?;

    let clock_in = entry
        .clock_in
        .ok_or_else(|| AppError::bad_request("No clock-in found for today"))?;

    if entry.clock_out.is_some() {
        return Err(AppError::bad_request("Already clocked out"));
    }

    let breakdown = calculate(clock_in, now, &settings);
    let shift = get_shift_for_date(pool, employee_id, today).await?;
    let attendance = evaluate_attendance(clock_in, Some(now), shift.as_ref(), &settings, today);

    let ot_status = if breakdown.ot_minutes > 0 {
        OtStatus::Pending
    } else {
        OtStatus::None
    };

    let updated = sqlx::query_as::<_, TimeEntry>(
        "UPDATE time_entries
         SET clock_out = $2,
             gross_minutes = $3,
             net_minutes = $4,
             regular_minutes = $5,
             ot_minutes = $6,
             ot_status = $7,
             attendance = $8
         WHERE id = $1
         RETURNING id, employee_id, work_date, clock_in, clock_out,
                   gross_minutes, net_minutes, regular_minutes, ot_minutes,
                   ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, attendance, created_at",
    )
    .bind(entry.id)
    .bind(now)
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

pub async fn list_entries_for_employee(
    pool: &PgPool,
    employee_id: Uuid,
    limit: i64,
) -> AppResult<Vec<TimeEntry>> {
    let entries = sqlx::query_as::<_, TimeEntry>(
        "SELECT id, employee_id, work_date, clock_in, clock_out,
                gross_minutes, net_minutes, regular_minutes, ot_minutes,
                ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, attendance, created_at
         FROM time_entries
         WHERE employee_id = $1
         ORDER BY work_date DESC
         LIMIT $2",
    )
    .bind(employee_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(entries)
}