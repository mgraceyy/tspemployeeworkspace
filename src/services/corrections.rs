use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::TimeEntry;
use crate::services::attendance::{evaluate_attendance, get_shift_for_date};
use crate::services::hours::calculate;
use crate::services::settings::get_settings;
use crate::services::team::assert_can_manage;

struct ApplyCorrectionInput<'a> {
    entry_id: Uuid,
    employee_id: Uuid,
    work_date: Date,
    editor_id: Uuid,
    old_clock_in: Option<OffsetDateTime>,
    old_clock_out: Option<OffsetDateTime>,
    new_clock_in: OffsetDateTime,
    new_clock_out: OffsetDateTime,
    reason: &'a str,
}

async fn apply_correction(pool: &PgPool, input: ApplyCorrectionInput<'_>) -> AppResult<TimeEntry> {
    let ApplyCorrectionInput {
        entry_id,
        employee_id,
        work_date,
        editor_id,
        old_clock_in,
        old_clock_out,
        new_clock_in,
        new_clock_out,
        reason,
    } = input;
    if new_clock_out <= new_clock_in {
        return Err(AppError::bad_request("Clock out must be after clock in"));
    }

    crate::services::payroll_controls::assert_work_date_editable(pool, work_date).await?;

    let settings = get_settings(pool).await?;
    let breakdown = calculate(new_clock_in, new_clock_out, &settings);
    let shift = get_shift_for_date(pool, employee_id, work_date).await?;
    let attendance = evaluate_attendance(
        new_clock_in,
        Some(new_clock_out),
        shift.as_ref(),
        &settings,
        work_date,
        &settings.timezone,
    )?;

    let ot_status = crate::services::clock::ot_status_for_minutes(breakdown.ot_minutes, &settings);

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
             ot_request_reason = NULL,
             attendance = $9
         WHERE id = $1
         RETURNING id, employee_id, work_date, clock_in, clock_out,
                   gross_minutes, net_minutes, regular_minutes, ot_minutes,
                   ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, ot_request_reason,
                   attendance, created_at",
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

pub struct CorrectionSubmission<'a> {
    pub editor_id: Uuid,
    pub manager_id: Uuid,
    pub is_admin: bool,
    pub new_clock_in: OffsetDateTime,
    pub new_clock_out: OffsetDateTime,
    pub reason: &'a str,
}

pub async fn correct_entry(
    pool: &PgPool,
    entry_id: Uuid,
    submission: &CorrectionSubmission<'_>,
) -> AppResult<TimeEntry> {
    let entry = sqlx::query_as::<_, TimeEntry>(
        "SELECT id, employee_id, work_date, clock_in, clock_out,
                gross_minutes, net_minutes, regular_minutes, ot_minutes,
                ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, ot_request_reason,
                attendance, created_at
         FROM time_entries
         WHERE id = $1",
    )
    .bind(entry_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::NotFound)?;

    assert_can_manage(
        pool,
        submission.manager_id,
        entry.employee_id,
        submission.is_admin,
    )
    .await?;

    apply_correction(
        pool,
        ApplyCorrectionInput {
            entry_id,
            employee_id: entry.employee_id,
            work_date: entry.work_date,
            editor_id: submission.editor_id,
            old_clock_in: entry.clock_in,
            old_clock_out: entry.clock_out,
            new_clock_in: submission.new_clock_in,
            new_clock_out: submission.new_clock_out,
            reason: submission.reason,
        },
    )
    .await
}

pub async fn create_corrected_entry(
    pool: &PgPool,
    employee_id: Uuid,
    work_date: Date,
    submission: &CorrectionSubmission<'_>,
) -> AppResult<TimeEntry> {
    assert_can_manage(
        pool,
        submission.manager_id,
        employee_id,
        submission.is_admin,
    )
    .await?;
    crate::services::payroll_controls::assert_work_date_editable(pool, work_date).await?;

    if let Some(existing) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM time_entries WHERE employee_id = $1 AND work_date = $2",
    )
    .bind(employee_id)
    .bind(work_date)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    {
        return correct_entry(pool, existing, submission).await;
    }

    if submission.new_clock_out <= submission.new_clock_in {
        return Err(AppError::bad_request("Clock out must be after clock in"));
    }

    let settings = get_settings(pool).await?;
    let breakdown = calculate(submission.new_clock_in, submission.new_clock_out, &settings);
    let shift = get_shift_for_date(pool, employee_id, work_date).await?;
    let attendance = evaluate_attendance(
        submission.new_clock_in,
        Some(submission.new_clock_out),
        shift.as_ref(),
        &settings,
        work_date,
        &settings.timezone,
    )?;

    let ot_status = crate::services::clock::ot_status_for_minutes(breakdown.ot_minutes, &settings);

    let entry = sqlx::query_as::<_, TimeEntry>(
        "INSERT INTO time_entries
            (employee_id, work_date, clock_in, clock_out, gross_minutes, net_minutes,
             regular_minutes, ot_minutes, ot_status, attendance)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING id, employee_id, work_date, clock_in, clock_out,
                   gross_minutes, net_minutes, regular_minutes, ot_minutes,
                   ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, ot_request_reason,
                   attendance, created_at",
    )
    .bind(employee_id)
    .bind(work_date)
    .bind(submission.new_clock_in)
    .bind(submission.new_clock_out)
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
    .bind(submission.editor_id)
    .bind(submission.reason)
    .bind(submission.new_clock_in)
    .bind(submission.new_clock_out)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(entry)
}

#[derive(Debug, sqlx::FromRow)]
pub struct CorrectionLogEntry {
    pub id: uuid::Uuid,
    pub employee_code: String,
    pub employee_name: String,
    pub work_date: Date,
    pub editor_name: String,
    pub reason: String,
    pub old_clock_in: Option<OffsetDateTime>,
    pub old_clock_out: Option<OffsetDateTime>,
    pub new_clock_in: Option<OffsetDateTime>,
    pub new_clock_out: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Default)]
pub struct CorrectionLogQuery {
    pub search: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

pub async fn count_correction_logs(pool: &PgPool, search: Option<&str>) -> AppResult<i64> {
    let pattern = crate::services::pagination::search_pattern(search);
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM correction_logs cl
         JOIN time_entries te ON te.id = cl.time_entry_id
         JOIN employees ee ON ee.id = te.employee_id
         JOIN employees ed ON ed.id = cl.edited_by
         WHERE ($1::text IS NULL OR (
             ee.employee_code ILIKE $1
             OR ee.full_name ILIKE $1
             OR ed.full_name ILIKE $1
             OR cl.reason ILIKE $1
         ))",
    )
    .bind(pattern)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(count)
}

pub async fn list_correction_logs(
    pool: &PgPool,
    query: &CorrectionLogQuery,
) -> AppResult<Vec<CorrectionLogEntry>> {
    let pattern = crate::services::pagination::search_pattern(query.search.as_deref());
    let rows = sqlx::query_as::<_, CorrectionLogEntry>(
        "SELECT cl.id,
                ee.employee_code,
                ee.full_name AS employee_name,
                te.work_date,
                ed.full_name AS editor_name,
                cl.reason,
                cl.old_clock_in,
                cl.old_clock_out,
                cl.new_clock_in,
                cl.new_clock_out,
                cl.created_at
         FROM correction_logs cl
         JOIN time_entries te ON te.id = cl.time_entry_id
         JOIN employees ee ON ee.id = te.employee_id
         JOIN employees ed ON ed.id = cl.edited_by
         WHERE ($1::text IS NULL OR (
             ee.employee_code ILIKE $1
             OR ee.full_name ILIKE $1
             OR ed.full_name ILIKE $1
             OR cl.reason ILIKE $1
         ))
         ORDER BY cl.created_at DESC
         LIMIT $2 OFFSET $3",
    )
    .bind(pattern)
    .bind(query.limit)
    .bind(query.offset)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}
