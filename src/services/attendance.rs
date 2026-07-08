use sqlx::{PgPool, Postgres, Transaction};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{AttendanceStatus, CompanySettings, ShiftTemplate};
use crate::services::team::assert_can_manage;

pub fn evaluate_attendance(
    clock_in: OffsetDateTime,
    clock_out: Option<OffsetDateTime>,
    shift: Option<&ShiftTemplate>,
    settings: &CompanySettings,
    work_date: Date,
    timezone: &str,
) -> AppResult<AttendanceStatus> {
    let Some(shift) = shift else {
        return Ok(AttendanceStatus::OnTime);
    };

    let shift_start = super::timezone::combine_date_time(work_date, shift.start_time, timezone)?;
    let mut shift_end = super::timezone::combine_date_time(work_date, shift.end_time, timezone)?;
    if shift.end_time <= shift.start_time {
        shift_end += time::Duration::days(1);
    }
    let grace_limit = shift_start + time::Duration::minutes(settings.grace_minutes as i64);

    if clock_in > grace_limit {
        return Ok(AttendanceStatus::Late);
    }

    if let Some(clock_out) = clock_out {
        let expected_minutes = (shift_end - shift_start).whole_minutes() as i32;
        let worked_minutes = (clock_out - clock_in).whole_minutes() as i32;
        if worked_minutes + 30 < expected_minutes {
            return Ok(AttendanceStatus::Partial);
        }
    }

    Ok(AttendanceStatus::OnTime)
}

pub async fn get_shift_for_date(
    pool: &PgPool,
    employee_id: Uuid,
    work_date: Date,
) -> AppResult<Option<ShiftTemplate>> {
    let day_of_week = work_date.weekday().number_days_from_sunday() as i16;
    let shift = sqlx::query_as::<_, ShiftTemplate>(
        "SELECT id, employee_id, day_of_week, start_time, end_time
         FROM shift_templates
         WHERE employee_id = $1 AND day_of_week = $2",
    )
    .bind(employee_id)
    .bind(day_of_week)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(shift)
}

pub async fn mark_absence_for_employee(
    pool: &PgPool,
    employee_id: Uuid,
    work_date: Date,
    status: AttendanceStatus,
    editor_id: Uuid,
    is_admin: bool,
    manager_id: Uuid,
) -> AppResult<()> {
    if !status.is_manager_markable() {
        return Err(AppError::bad_request("Invalid absence type"));
    }

    assert_can_manage(pool, manager_id, employee_id, is_admin).await?;
    crate::services::payroll_controls::assert_work_date_editable(pool, work_date).await?;
    write_absence_entry_pool(pool, employee_id, work_date, status, editor_id).await
}

pub async fn clear_leave_absence_for_employee_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    employee_id: Uuid,
    work_date: Date,
    status: AttendanceStatus,
) -> AppResult<()> {
    sqlx::query(
        "DELETE FROM time_entries
         WHERE employee_id = $1 AND work_date = $2
           AND clock_in IS NULL AND clock_out IS NULL
           AND attendance = $3",
    )
    .bind(employee_id)
    .bind(work_date)
    .bind(status)
    .execute(&mut **tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

pub async fn mark_absence_for_employee_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    employee_id: Uuid,
    work_date: Date,
    status: AttendanceStatus,
    editor_id: Uuid,
) -> AppResult<()> {
    if !status.is_manager_markable() {
        return Err(AppError::bad_request("Invalid absence type"));
    }
    write_absence_entry_tx(tx, employee_id, work_date, status, editor_id).await
}

async fn write_absence_entry_pool(
    pool: &PgPool,
    employee_id: Uuid,
    work_date: Date,
    status: AttendanceStatus,
    editor_id: Uuid,
) -> AppResult<()> {
    let existing: Option<(Uuid, Option<time::OffsetDateTime>)> = sqlx::query_as(
        "SELECT id, clock_in FROM time_entries WHERE employee_id = $1 AND work_date = $2",
    )
    .bind(employee_id)
    .bind(work_date)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    apply_absence_write(pool, employee_id, work_date, status, editor_id, existing).await
}

async fn write_absence_entry_tx(
    tx: &mut Transaction<'_, Postgres>,
    employee_id: Uuid,
    work_date: Date,
    status: AttendanceStatus,
    editor_id: Uuid,
) -> AppResult<()> {
    let existing: Option<(Uuid, Option<time::OffsetDateTime>)> = sqlx::query_as(
        "SELECT id, clock_in FROM time_entries WHERE employee_id = $1 AND work_date = $2",
    )
    .bind(employee_id)
    .bind(work_date)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    if let Some((_, Some(_))) = existing {
        return Err(AppError::bad_request(
            "Cannot mark absence when employee already clocked in",
        ));
    }
    if let Some((entry_id, _)) = existing {
        sqlx::query("UPDATE time_entries SET attendance = $2 WHERE id = $1")
            .bind(entry_id)
            .bind(status)
            .execute(&mut **tx)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    } else {
        sqlx::query(
            "INSERT INTO time_entries (employee_id, work_date, attendance)
             VALUES ($1, $2, $3)",
        )
        .bind(employee_id)
        .bind(work_date)
        .bind(status)
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    }
    tracing::info!(
        employee_id = %employee_id,
        work_date = %work_date,
        editor_id = %editor_id,
        ?status,
        "marked absence"
    );
    Ok(())
}

async fn apply_absence_write(
    pool: &PgPool,
    employee_id: Uuid,
    work_date: Date,
    status: AttendanceStatus,
    editor_id: Uuid,
    existing: Option<(Uuid, Option<time::OffsetDateTime>)>,
) -> AppResult<()> {
    if let Some((_, Some(_))) = existing {
        return Err(AppError::bad_request(
            "Cannot mark absence when employee already clocked in",
        ));
    }
    if let Some((entry_id, _)) = existing {
        sqlx::query("UPDATE time_entries SET attendance = $2 WHERE id = $1")
            .bind(entry_id)
            .bind(status)
            .execute(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    } else {
        sqlx::query(
            "INSERT INTO time_entries (employee_id, work_date, attendance)
             VALUES ($1, $2, $3)",
        )
        .bind(employee_id)
        .bind(work_date)
        .bind(status)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    }
    tracing::info!(
        employee_id = %employee_id,
        work_date = %work_date,
        editor_id = %editor_id,
        ?status,
        "marked absence"
    );
    Ok(())
}

pub async fn mark_no_show_for_employee(
    pool: &PgPool,
    employee_id: Uuid,
    work_date: Date,
    editor_id: Uuid,
    is_admin: bool,
    manager_id: Uuid,
) -> AppResult<()> {
    mark_absence_for_employee(
        pool,
        employee_id,
        work_date,
        AttendanceStatus::NoShow,
        editor_id,
        is_admin,
        manager_id,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::settings::CompanySettings;
    use crate::services::timezone::combine_date_time;
    use time::{Month, Time};

    fn test_settings() -> CompanySettings {
        CompanySettings {
            company_name: "Test".into(),
            ..Default::default()
        }
    }

    fn shift(start_h: u8, end_h: u8) -> ShiftTemplate {
        ShiftTemplate {
            id: Uuid::new_v4(),
            employee_id: Uuid::new_v4(),
            day_of_week: 1,
            start_time: Time::from_hms(start_h, 0, 0).unwrap(),
            end_time: Time::from_hms(end_h, 0, 0).unwrap(),
        }
    }

    #[test]
    fn on_time_when_clock_in_within_grace() {
        let settings = test_settings();
        let work_date = Date::from_calendar_date(2026, Month::June, 15).unwrap();
        let shift = shift(8, 17);
        let clock_in =
            combine_date_time(work_date, Time::from_hms(8, 3, 0).unwrap(), "Asia/Manila").unwrap();
        let status = evaluate_attendance(
            clock_in,
            None,
            Some(&shift),
            &settings,
            work_date,
            "Asia/Manila",
        )
        .unwrap();
        assert_eq!(status, AttendanceStatus::OnTime);
    }

    #[test]
    fn late_when_clock_in_after_grace() {
        let settings = test_settings();
        let work_date = Date::from_calendar_date(2026, Month::June, 15).unwrap();
        let shift = shift(8, 17);
        let clock_in =
            combine_date_time(work_date, Time::from_hms(8, 10, 0).unwrap(), "Asia/Manila").unwrap();
        let status = evaluate_attendance(
            clock_in,
            None,
            Some(&shift),
            &settings,
            work_date,
            "Asia/Manila",
        )
        .unwrap();
        assert_eq!(status, AttendanceStatus::Late);
    }

    #[test]
    fn on_time_for_overnight_shift_within_grace() {
        let settings = test_settings();
        let work_date = Date::from_calendar_date(2026, Month::June, 15).unwrap();
        let shift = ShiftTemplate {
            id: Uuid::new_v4(),
            employee_id: Uuid::new_v4(),
            day_of_week: 1,
            start_time: Time::from_hms(22, 0, 0).unwrap(),
            end_time: Time::from_hms(6, 0, 0).unwrap(),
        };
        let clock_in =
            combine_date_time(work_date, Time::from_hms(22, 3, 0).unwrap(), "Asia/Manila").unwrap();
        let clock_out = combine_date_time(
            work_date + time::Duration::days(1),
            Time::from_hms(6, 0, 0).unwrap(),
            "Asia/Manila",
        )
        .unwrap();
        let status = evaluate_attendance(
            clock_in,
            Some(clock_out),
            Some(&shift),
            &settings,
            work_date,
            "Asia/Manila",
        )
        .unwrap();
        assert_eq!(status, AttendanceStatus::OnTime);
    }

    #[test]
    fn partial_when_shift_not_fully_worked() {
        let settings = test_settings();
        let work_date = Date::from_calendar_date(2026, Month::June, 15).unwrap();
        let shift = shift(8, 17);
        let clock_in =
            combine_date_time(work_date, Time::from_hms(8, 0, 0).unwrap(), "Asia/Manila").unwrap();
        let clock_out =
            combine_date_time(work_date, Time::from_hms(14, 0, 0).unwrap(), "Asia/Manila").unwrap();
        let status = evaluate_attendance(
            clock_in,
            Some(clock_out),
            Some(&shift),
            &settings,
            work_date,
            "Asia/Manila",
        )
        .unwrap();
        assert_eq!(status, AttendanceStatus::Partial);
    }
}
