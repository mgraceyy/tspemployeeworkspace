use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{OtStatus, TimeEntry};
use crate::services::attendance::{evaluate_attendance, get_shift_for_date};
use crate::services::hours::calculate;
use crate::services::payroll_controls::assert_work_date_editable;
use crate::services::settings::get_settings;
use crate::services::timezone::{company_date_now, now_company};

pub fn ot_status_for_minutes(
    ot_minutes: i32,
    settings: &crate::models::CompanySettings,
) -> OtStatus {
    if ot_minutes > 0 {
        if settings.ot_requires_approval {
            OtStatus::Pending
        } else {
            OtStatus::Approved
        }
    } else {
        OtStatus::None
    }
}

pub async fn get_today_entry(pool: &PgPool, employee_id: Uuid) -> AppResult<Option<TimeEntry>> {
    let settings = get_settings(pool).await?;
    let today = company_date_now(&settings)?;
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
                ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, ot_request_reason,
                attendance, created_at
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
    let settings = get_settings(pool).await?;
    let today = company_date_now(&settings)?;
    let now = now_company(&settings)?;
    assert_work_date_editable(pool, today).await?;

    if let Some(existing) = get_entry_for_date(pool, employee_id, today).await? {
        if existing.clock_in.is_some() && existing.clock_out.is_none() {
            return Err(AppError::bad_request("Already clocked in"));
        }
        if existing.clock_out.is_some() {
            return Err(AppError::bad_request("Already completed today"));
        }
    }

    let shift = get_shift_for_date(pool, employee_id, today).await?;
    let attendance = evaluate_attendance(
        now,
        None,
        shift.as_ref(),
        &settings,
        today,
        &settings.timezone,
    )?;

    let entry = sqlx::query_as::<_, TimeEntry>(
        "INSERT INTO time_entries (employee_id, work_date, clock_in, attendance)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (employee_id, work_date)
         DO UPDATE SET clock_in = EXCLUDED.clock_in, attendance = EXCLUDED.attendance
         RETURNING id, employee_id, work_date, clock_in, clock_out,
                   gross_minutes, net_minutes, regular_minutes, ot_minutes,
                   ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, ot_request_reason,
                attendance, created_at",
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

pub async fn clock_out(
    pool: &PgPool,
    employee_id: Uuid,
    ot_request_reason: Option<&str>,
) -> AppResult<TimeEntry> {
    let settings = get_settings(pool).await?;
    let today = company_date_now(&settings)?;
    let now = now_company(&settings)?;
    assert_work_date_editable(pool, today).await?;

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
    let attendance = evaluate_attendance(
        clock_in,
        Some(now),
        shift.as_ref(),
        &settings,
        today,
        &settings.timezone,
    )?;

    let ot_status = ot_status_for_minutes(breakdown.ot_minutes, &settings);
    let trimmed_reason = ot_request_reason
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if breakdown.ot_minutes > 0 && settings.ot_requires_approval && trimmed_reason.is_none() {
        return Err(AppError::bad_request(
            "Please provide a reason for overtime — it is required for manager approval",
        ));
    }

    let updated = sqlx::query_as::<_, TimeEntry>(
        "UPDATE time_entries
         SET clock_out = $2,
             gross_minutes = $3,
             net_minutes = $4,
             regular_minutes = $5,
             ot_minutes = $6,
             ot_status = $7,
             attendance = $8,
             ot_request_reason = $9
         WHERE id = $1
         RETURNING id, employee_id, work_date, clock_in, clock_out,
                   gross_minutes, net_minutes, regular_minutes, ot_minutes,
                   ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, ot_request_reason,
                   attendance, created_at",
    )
    .bind(entry.id)
    .bind(now)
    .bind(breakdown.gross_minutes)
    .bind(breakdown.net_minutes)
    .bind(breakdown.regular_minutes)
    .bind(breakdown.ot_minutes)
    .bind(ot_status)
    .bind(attendance)
    .bind(trimmed_reason)
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
                ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, ot_request_reason,
                attendance, created_at
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

pub async fn list_entries_for_employee_range(
    pool: &PgPool,
    employee_id: Uuid,
    start: Date,
    end: Date,
) -> AppResult<Vec<TimeEntry>> {
    let entries = sqlx::query_as::<_, TimeEntry>(
        "SELECT id, employee_id, work_date, clock_in, clock_out,
                gross_minutes, net_minutes, regular_minutes, ot_minutes,
                ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, ot_request_reason,
                attendance, created_at
         FROM time_entries
         WHERE employee_id = $1 AND work_date BETWEEN $2 AND $3
         ORDER BY work_date DESC",
    )
    .bind(employee_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(entries)
}
