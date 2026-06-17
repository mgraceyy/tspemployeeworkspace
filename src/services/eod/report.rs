use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{EodHistoryItem, EodReport, EodReportStatus, EodReportSummary, EodTask};
use crate::services::payroll_controls::assert_work_date_editable;
use crate::services::settings::get_settings;
use crate::services::timezone::company_date_now;

use super::reminder::clocked_in_on_date;
use super::tasks::{list_tasks, EodTaskInput};

pub async fn get_report(
    pool: &PgPool,
    employee_id: Uuid,
    report_date: Date,
) -> AppResult<Option<EodReport>> {
    let row = sqlx::query_as::<_, EodReport>(
        "SELECT id, employee_id, report_date, summary, status, submitted_at
         FROM eod_reports
         WHERE employee_id = $1 AND report_date = $2",
    )
    .bind(employee_id)
    .bind(report_date)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(row)
}

pub async fn save_report(
    pool: &PgPool,
    employee_id: Uuid,
    report_date: Date,
    summary: &str,
    submit: bool,
    tasks: &[EodTaskInput],
) -> AppResult<EodReport> {
    if !clocked_in_on_date(pool, employee_id, report_date).await? {
        return Err(AppError::bad_request(
            "EOD is only available on days you clocked in",
        ));
    }

    let settings = get_settings(pool).await?;
    let today = company_date_now(&settings)?;
    if report_date != today {
        return Err(AppError::bad_request("EOD can only be edited for today"));
    }
    assert_work_date_editable(pool, report_date).await?;

    let existing = get_report(pool, employee_id, report_date).await?;
    if let Some(ref report) = existing {
        if report.status == EodReportStatus::Submitted {
            return Err(AppError::bad_request("Submitted EOD cannot be edited"));
        }
    }

    let status = if submit {
        EodReportStatus::Submitted
    } else {
        EodReportStatus::Draft
    };

    let report = if let Some(existing) = existing {
        sqlx::query_as::<_, EodReport>(
            "UPDATE eod_reports
             SET summary = $2,
                 status = $3,
                 submitted_at = CASE WHEN $4 THEN now() ELSE submitted_at END,
                 updated_at = now()
             WHERE id = $1
             RETURNING id, employee_id, report_date, summary, status, submitted_at",
        )
        .bind(existing.id)
        .bind(summary.trim())
        .bind(status)
        .bind(submit)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
    } else {
        sqlx::query_as::<_, EodReport>(
            "INSERT INTO eod_reports (employee_id, report_date, summary, status, submitted_at)
             VALUES ($1, $2, $3, $4, CASE WHEN $5 THEN now() ELSE NULL END)
             RETURNING id, employee_id, report_date, summary, status, submitted_at",
        )
        .bind(employee_id)
        .bind(report_date)
        .bind(summary.trim())
        .bind(status)
        .bind(submit)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
    };

    sqlx::query("DELETE FROM eod_tasks WHERE eod_report_id = $1")
        .bind(report.id)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    for (index, task) in tasks.iter().enumerate() {
        if task.title.trim().is_empty() {
            continue;
        }
        sqlx::query(
            "INSERT INTO eod_tasks (eod_report_id, kind, title, sort_order)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(report.id)
        .bind(task.kind)
        .bind(task.title.trim())
        .bind(index as i32)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    }

    Ok(report)
}

pub async fn get_report_with_tasks(
    pool: &PgPool,
    employee_id: Uuid,
    report_date: Date,
) -> AppResult<(Option<EodReport>, Vec<EodTask>)> {
    let report = get_report(pool, employee_id, report_date).await?;
    let tasks = if let Some(ref r) = report {
        list_tasks(pool, r.id).await?
    } else {
        Vec::new()
    };
    Ok((report, tasks))
}

pub async fn list_employee_eod_history(
    pool: &PgPool,
    employee_id: Uuid,
    limit: i64,
) -> AppResult<Vec<EodHistoryItem>> {
    let rows = sqlx::query_as::<_, EodHistoryItem>(
        "SELECT id, report_date, summary, submitted_at
         FROM eod_reports
         WHERE employee_id = $1 AND status = 'submitted'
         ORDER BY report_date DESC
         LIMIT $2",
    )
    .bind(employee_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn list_today_submitted_eod(pool: &PgPool) -> AppResult<Vec<EodReportSummary>> {
    let settings = get_settings(pool).await?;
    let today = company_date_now(&settings)?;
    let rows = sqlx::query_as::<_, EodReportSummary>(
        "SELECT er.id, er.employee_id, e.employee_code, e.full_name, p.department,
                er.report_date, er.summary, er.status, er.submitted_at
         FROM eod_reports er
         JOIN employees e ON e.id = er.employee_id
         JOIN employee_profiles p ON p.employee_id = e.id
         WHERE er.report_date = $1 AND er.status = 'submitted'
         ORDER BY e.full_name",
    )
    .bind(today)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn unlock_report(pool: &PgPool, report_id: Uuid, admin_id: Uuid) -> AppResult<EodReport> {
    let settings = get_settings(pool).await?;
    let today = company_date_now(&settings)?;
    assert_work_date_editable(pool, today).await?;
    let report = sqlx::query_as::<_, EodReport>(
        "UPDATE eod_reports
         SET status = 'draft',
             submitted_at = NULL,
             unlocked_at = now(),
             unlocked_by = $2,
             updated_at = now()
         WHERE id = $1
           AND report_date = $3
           AND status = 'submitted'
         RETURNING id, employee_id, report_date, summary, status, submitted_at",
    )
    .bind(report_id)
    .bind(admin_id)
    .bind(today)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or_else(|| AppError::bad_request("Only today's submitted EOD reports can be unlocked"))?;
    Ok(report)
}
