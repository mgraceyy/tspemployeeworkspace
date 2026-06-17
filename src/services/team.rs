use sqlx::PgPool;
use time::{Date, OffsetDateTime, Time};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{AttendanceStatus, EmployeeSummary, TimeEntry};
use crate::services::timezone::{combine_date_time, manila_date_now, now_manila};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TeamAttendanceRow {
    pub employee_id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub entry_id: Option<Uuid>,
    pub clock_in: Option<OffsetDateTime>,
    pub clock_out: Option<OffsetDateTime>,
    pub attendance: Option<AttendanceStatus>,
    pub shift_start: Option<Time>,
    pub shift_end: Option<Time>,
}

#[derive(Debug, Clone)]
pub struct TeamMemberStatus {
    pub employee_id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub entry_id: Option<Uuid>,
    pub clock_in: Option<OffsetDateTime>,
    pub clock_out: Option<OffsetDateTime>,
    pub attendance: Option<AttendanceStatus>,
    pub shift_start: Option<Time>,
    pub shift_end: Option<Time>,
    pub status: String,
    pub can_mark_no_show: bool,
}

pub async fn assert_can_manage(
    pool: &PgPool,
    actor_id: Uuid,
    employee_id: Uuid,
    is_admin: bool,
) -> AppResult<()> {
    if is_admin {
        return Ok(());
    }

    let manager_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT manager_id FROM employees WHERE id = $1 AND is_active = TRUE",
    )
    .bind(employee_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::NotFound)?;

    if manager_id == Some(actor_id) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

pub async fn list_manageable_employees(
    pool: &PgPool,
    manager_id: Uuid,
    is_admin: bool,
) -> AppResult<Vec<EmployeeSummary>> {
    let employees = if is_admin {
        sqlx::query_as::<_, EmployeeSummary>(
            "SELECT id, employee_code, full_name, role, manager_id, is_active
             FROM employees
             WHERE is_active = TRUE AND role = 'employee'
             ORDER BY full_name",
        )
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, EmployeeSummary>(
            "SELECT id, employee_code, full_name, role, manager_id, is_active
             FROM employees
             WHERE manager_id = $1 AND is_active = TRUE
             ORDER BY full_name",
        )
        .bind(manager_id)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(employees)
}

pub async fn list_team_attendance_today(
    pool: &PgPool,
    manager_id: Uuid,
    is_admin: bool,
) -> AppResult<Vec<TeamMemberStatus>> {
    let today = manila_date_now();
    let day_of_week = today.weekday().number_days_from_sunday() as i16;
    let now = now_manila();

    let rows = if is_admin {
        sqlx::query_as::<_, TeamAttendanceRow>(
            "SELECT e.id AS employee_id, e.employee_code, e.full_name,
                    te.id AS entry_id, te.clock_in, te.clock_out, te.attendance,
                    st.start_time AS shift_start, st.end_time AS shift_end
             FROM employees e
             LEFT JOIN time_entries te
               ON te.employee_id = e.id AND te.work_date = $1
             LEFT JOIN shift_templates st
               ON st.employee_id = e.id AND st.day_of_week = $2
             WHERE e.is_active = TRUE AND e.role = 'employee'
             ORDER BY e.full_name",
        )
        .bind(today)
        .bind(day_of_week)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, TeamAttendanceRow>(
            "SELECT e.id AS employee_id, e.employee_code, e.full_name,
                    te.id AS entry_id, te.clock_in, te.clock_out, te.attendance,
                    st.start_time AS shift_start, st.end_time AS shift_end
             FROM employees e
             LEFT JOIN time_entries te
               ON te.employee_id = e.id AND te.work_date = $1
             LEFT JOIN shift_templates st
               ON st.employee_id = e.id AND st.day_of_week = $2
             WHERE e.is_active = TRUE AND e.manager_id = $3
             ORDER BY e.full_name",
        )
        .bind(today)
        .bind(day_of_week)
        .bind(manager_id)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let status = derive_status(&row, today, now);
            let can_mark_no_show = row.clock_in.is_none()
                && row.attendance != Some(AttendanceStatus::NoShow);
            TeamMemberStatus {
                employee_id: row.employee_id,
                employee_code: row.employee_code,
                full_name: row.full_name,
                entry_id: row.entry_id,
                clock_in: row.clock_in,
                clock_out: row.clock_out,
                attendance: row.attendance,
                shift_start: row.shift_start,
                shift_end: row.shift_end,
                status,
                can_mark_no_show,
            }
        })
        .collect())
}

fn derive_status(row: &TeamAttendanceRow, work_date: Date, now: OffsetDateTime) -> String {
    if row.attendance == Some(AttendanceStatus::NoShow) {
        return "no_show".into();
    }
    if row.attendance == Some(AttendanceStatus::Absent) {
        return "absent".into();
    }
    match (row.clock_in, row.clock_out) {
        (None, _) => {
            if let (Some(start), Some(end)) = (row.shift_start, row.shift_end) {
                let shift_end = combine_date_time(work_date, end);
                if now > shift_end {
                    return "absent".into();
                }
                let _ = start;
            }
            "not_started".into()
        }
        (Some(_), None) => "clocked_in".into(),
        (Some(_), Some(_)) => "completed".into(),
    }
}

pub async fn get_employee_summary(
    pool: &PgPool,
    employee_id: Uuid,
) -> AppResult<EmployeeSummary> {
    sqlx::query_as::<_, EmployeeSummary>(
        "SELECT id, employee_code, full_name, role, manager_id, is_active
         FROM employees WHERE id = $1",
    )
    .bind(employee_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::NotFound)
}

pub async fn get_entry_if_manageable(
    pool: &PgPool,
    entry_id: Uuid,
    actor_id: Uuid,
    is_admin: bool,
) -> AppResult<TimeEntry> {
    let entry = sqlx::query_as::<_, TimeEntry>(
        "SELECT id, employee_id, work_date, clock_in, clock_out,
                gross_minutes, net_minutes, regular_minutes, ot_minutes,
                ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, attendance, created_at
         FROM time_entries WHERE id = $1",
    )
    .bind(entry_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::NotFound)?;

    assert_can_manage(pool, actor_id, entry.employee_id, is_admin).await?;
    Ok(entry)
}