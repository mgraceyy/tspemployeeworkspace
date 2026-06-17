use std::collections::HashMap;

use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{EodReportStatus, EodReportSummary, EodTask, EodTaskKind};
use crate::services::settings::get_settings;
use crate::services::timezone::{company_date_now, format_date, format_time};

#[derive(Debug, sqlx::FromRow)]
pub struct TeamEodStatus {
    pub employee_id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub clocked_in: bool,
    pub eod_status: Option<EodReportStatus>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct EodExportRow {
    pub report_id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub department: Option<String>,
    pub report_date: Date,
    pub summary: String,
    pub status: EodReportStatus,
    pub submitted_at: Option<time::OffsetDateTime>,
}

pub async fn list_department_eod(
    pool: &PgPool,
    employee_id: Uuid,
    department: &str,
    report_date: Date,
) -> AppResult<Vec<EodReportSummary>> {
    let rows = sqlx::query_as::<_, EodReportSummary>(
        "SELECT er.id, er.employee_id, e.employee_code, e.full_name, p.department,
                er.report_date, er.summary, er.status, er.submitted_at
         FROM eod_reports er
         JOIN employees e ON e.id = er.employee_id
         JOIN employee_profiles p ON p.employee_id = e.id
         WHERE p.department = $1
           AND er.report_date = $2
           AND er.status = 'submitted'
           AND e.is_active = TRUE
         ORDER BY e.full_name",
    )
    .bind(department)
    .bind(report_date)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(rows
        .into_iter()
        .filter(|r| r.employee_id != employee_id)
        .collect())
}

pub async fn list_department_eod_recent(
    pool: &PgPool,
    department: &str,
    since: Date,
) -> AppResult<Vec<EodReportSummary>> {
    let rows = sqlx::query_as::<_, EodReportSummary>(
        "SELECT er.id, er.employee_id, e.employee_code, e.full_name, p.department,
                er.report_date, er.summary, er.status, er.submitted_at
         FROM eod_reports er
         JOIN employees e ON e.id = er.employee_id
         JOIN employee_profiles p ON p.employee_id = e.id
         WHERE p.department = $1
           AND er.report_date >= $2
           AND er.status = 'submitted'
           AND e.is_active = TRUE
         ORDER BY er.report_date DESC, e.full_name",
    )
    .bind(department)
    .bind(since)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn count_missing_team_eod(
    pool: &PgPool,
    manager_id: Uuid,
    is_admin: bool,
) -> AppResult<i64> {
    let settings = get_settings(pool).await?;
    let today = company_date_now(&settings)?;
    let rows = list_team_eod_status(pool, manager_id, is_admin, today).await?;
    Ok(rows
        .iter()
        .filter(|r| r.clocked_in && r.eod_status != Some(EodReportStatus::Submitted))
        .count() as i64)
}

pub async fn list_team_eod_status(
    pool: &PgPool,
    manager_id: Uuid,
    is_admin: bool,
    report_date: Date,
) -> AppResult<Vec<TeamEodStatus>> {
    let rows = if is_admin {
        sqlx::query_as::<_, TeamEodStatus>(
            "SELECT e.id AS employee_id, e.employee_code, e.full_name,
                    (te.clock_in IS NOT NULL) AS clocked_in,
                    er.status AS eod_status
             FROM employees e
             LEFT JOIN time_entries te
               ON te.employee_id = e.id AND te.work_date = $2
             LEFT JOIN eod_reports er
               ON er.employee_id = e.id AND er.report_date = $2
             WHERE e.is_active = TRUE AND e.role = 'employee'
             ORDER BY e.full_name",
        )
        .bind(report_date)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, TeamEodStatus>(
            "SELECT e.id AS employee_id, e.employee_code, e.full_name,
                    (te.clock_in IS NOT NULL) AS clocked_in,
                    er.status AS eod_status
             FROM employees e
             LEFT JOIN time_entries te
               ON te.employee_id = e.id AND te.work_date = $2
             LEFT JOIN eod_reports er
               ON er.employee_id = e.id AND er.report_date = $2
             WHERE e.manager_id = $1 AND e.is_active = TRUE
             ORDER BY e.full_name",
        )
        .bind(manager_id)
        .bind(report_date)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(rows)
}

pub async fn list_team_eod_export_rows(
    pool: &PgPool,
    manager_id: Uuid,
    is_admin: bool,
    since: Date,
    until: Date,
) -> AppResult<Vec<EodExportRow>> {
    let rows = if is_admin {
        sqlx::query_as::<_, EodExportRow>(
            "SELECT er.id AS report_id, e.employee_code, e.full_name, p.department,
                    er.report_date, er.summary, er.status, er.submitted_at
             FROM eod_reports er
             JOIN employees e ON e.id = er.employee_id
             LEFT JOIN employee_profiles p ON p.employee_id = e.id
             WHERE er.report_date BETWEEN $1 AND $2
               AND e.is_active = TRUE
               AND e.role = 'employee'
             ORDER BY er.report_date DESC, e.full_name",
        )
        .bind(since)
        .bind(until)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, EodExportRow>(
            "SELECT er.id AS report_id, e.employee_code, e.full_name, p.department,
                    er.report_date, er.summary, er.status, er.submitted_at
             FROM eod_reports er
             JOIN employees e ON e.id = er.employee_id
             LEFT JOIN employee_profiles p ON p.employee_id = e.id
             WHERE er.report_date BETWEEN $1 AND $2
               AND e.manager_id = $3
               AND e.is_active = TRUE
             ORDER BY er.report_date DESC, e.full_name",
        )
        .bind(since)
        .bind(until)
        .bind(manager_id)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn build_eod_weekly_csv(pool: &PgPool, rows: &[EodExportRow]) -> AppResult<Vec<u8>> {
    let settings = get_settings(pool).await?;
    let timezone = settings.timezone.as_str();
    let report_ids: Vec<Uuid> = rows.iter().map(|r| r.report_id).collect();
    let tasks = if report_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as::<_, EodTask>(
            "SELECT id, eod_report_id, kind, title, description, sort_order
             FROM eod_tasks
             WHERE eod_report_id = ANY($1)
             ORDER BY eod_report_id, kind, sort_order",
        )
        .bind(&report_ids)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
    };

    let mut tasks_by_report: HashMap<Uuid, Vec<&EodTask>> = HashMap::new();
    for task in &tasks {
        tasks_by_report
            .entry(task.eod_report_id)
            .or_default()
            .push(task);
    }

    let mut csv_bytes = Vec::new();
    {
        let mut writer = csv::Writer::from_writer(&mut csv_bytes);
        writer
            .write_record([
                "Employee Code",
                "Name",
                "Department",
                "Date",
                "Status",
                "Summary",
                "Completed",
                "Pending",
                "Blocked",
                "Planned",
                "Submitted At",
            ])
            .map_err(|e| AppError::Internal(e.into()))?;

        for row in rows {
            let report_tasks = tasks_by_report.get(&row.report_id);
            let (completed, pending, blocked, planned) = report_tasks
                .map(|ts| {
                    let mut c = Vec::new();
                    let mut p = Vec::new();
                    let mut b = Vec::new();
                    let mut pl = Vec::new();
                    for t in ts.iter() {
                        match t.kind {
                            EodTaskKind::Completed => c.push(t.title.as_str()),
                            EodTaskKind::Pending => p.push(t.title.as_str()),
                            EodTaskKind::Blocked => b.push(t.title.as_str()),
                            EodTaskKind::Planned => pl.push(t.title.as_str()),
                        }
                    }
                    (c.join("; "), p.join("; "), b.join("; "), pl.join("; "))
                })
                .unwrap_or_default();

            let status = match row.status {
                EodReportStatus::Draft => "Draft",
                EodReportStatus::Submitted => "Submitted",
            };
            let submitted = row
                .submitted_at
                .map(|dt| format_time(dt, timezone))
                .unwrap_or_default();

            writer
                .write_record([
                    row.employee_code.clone(),
                    row.full_name.clone(),
                    row.department.clone().unwrap_or_default(),
                    format_date(row.report_date),
                    status.to_string(),
                    row.summary.clone(),
                    completed,
                    pending,
                    blocked,
                    planned,
                    submitted,
                ])
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        writer.flush().map_err(|e| AppError::Internal(e.into()))?;
    }
    Ok(csv_bytes)
}
