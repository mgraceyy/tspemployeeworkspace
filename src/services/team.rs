use sqlx::PgPool;
use time::{Date, OffsetDateTime, Time};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{AttendanceStatus, EmployeeSummary, TimeEntry};
use crate::services::holidays::is_holiday;
use crate::services::settings::get_settings;
use crate::services::timezone::{combine_date_time, company_date_now, now_company};

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
    pub shift_note: Option<String>,
    pub can_mark_no_show: bool,
    pub can_mark_absence: bool,
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

    let manager_id: Option<Uuid> =
        sqlx::query_scalar("SELECT manager_id FROM employees WHERE id = $1 AND is_active = TRUE")
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
    grace_minutes: i32,
) -> AppResult<Vec<TeamMemberStatus>> {
    let settings = get_settings(pool).await?;
    let today = company_date_now(&settings)?;
    let day_of_week = today.weekday().number_days_from_sunday() as i16;
    let now = now_company(&settings)?;
    let holiday_today = is_holiday(pool, today).await?;
    let timezone = settings.timezone.clone();

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
            let status = derive_status(&row, today, now, holiday_today, &timezone);
            let shift_note = compute_shift_note(&row, today, now, grace_minutes, &timezone);
            let absence_marked = row
                .attendance
                .is_some_and(AttendanceStatus::is_manager_markable);
            let can_mark_absence = row.clock_in.is_none() && !absence_marked;
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
                shift_note,
                can_mark_no_show: can_mark_absence,
                can_mark_absence,
            }
        })
        .collect())
}

fn compute_shift_note(
    row: &TeamAttendanceRow,
    work_date: Date,
    now: OffsetDateTime,
    grace_minutes: i32,
    timezone: &str,
) -> Option<String> {
    let (Some(start), Some(end)) = (row.shift_start, row.shift_end) else {
        return Some("No shift scheduled".into());
    };
    let shift_start = combine_date_time(work_date, start, timezone).ok()?;
    let grace_limit = shift_start + time::Duration::minutes(grace_minutes as i64);
    let shift_label = format!(
        "{:02}:{:02}–{:02}:{:02}",
        start.hour(),
        start.minute(),
        end.hour(),
        end.minute()
    );

    if let Some(clock_in) = row.clock_in {
        if clock_in > grace_limit {
            let late_mins = (clock_in - shift_start).whole_minutes();
            return Some(format!("Late ({late_mins} min) · shift {shift_label}"));
        }
        return Some(format!("On time · shift {shift_label}"));
    }

    if row.attendance.is_none() && now > grace_limit {
        return Some(format!("Missing punch · shift {shift_label}"));
    }

    Some(format!("Shift {shift_label}"))
}

fn derive_status(
    row: &TeamAttendanceRow,
    work_date: Date,
    now: OffsetDateTime,
    holiday_today: bool,
    timezone: &str,
) -> String {
    if row.attendance == Some(AttendanceStatus::Late) {
        return "late".into();
    }
    if row.attendance == Some(AttendanceStatus::Partial) {
        return "partial".into();
    }
    if row.attendance == Some(AttendanceStatus::OnTime) && row.clock_out.is_some() {
        return "completed".into();
    }
    if row.attendance == Some(AttendanceStatus::SickLeave) {
        return "sick_leave".into();
    }
    if row.attendance == Some(AttendanceStatus::Vacation) {
        return "vacation".into();
    }
    if row.attendance == Some(AttendanceStatus::OfficialLeave) {
        return "official_leave".into();
    }
    if row.attendance == Some(AttendanceStatus::Offset) {
        return "offset".into();
    }
    if row.attendance == Some(AttendanceStatus::NoShow) {
        return "no_show".into();
    }
    if row.attendance == Some(AttendanceStatus::Absent) {
        return "absent".into();
    }
    if holiday_today && row.clock_in.is_none() {
        return "holiday".into();
    }
    match (row.clock_in, row.clock_out) {
        (None, _) => {
            if let (Some(start), Some(end)) = (row.shift_start, row.shift_end) {
                let shift_end = combine_date_time(work_date, end, timezone).unwrap_or(now);
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

pub async fn get_employee_summary(pool: &PgPool, employee_id: Uuid) -> AppResult<EmployeeSummary> {
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
                ot_status, ot_reviewed_by, ot_reviewed_at, ot_note, ot_request_reason,
                attendance, created_at
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
