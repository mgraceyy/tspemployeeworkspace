//! Vacation and sick-leave balance tracking (stored as tenths of a day).

use sqlx::{PgPool, Postgres, Transaction};
use time::Date;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{LeaveDayPortion, LeaveRequestType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaveBalanceSnapshot {
    pub vacation_tenths: i32,
    pub sick_tenths: i32,
}

pub fn format_leave_balance(tenths: i32) -> String {
    if tenths % 10 == 0 {
        (tenths / 10).to_string()
    } else {
        format!("{:.1}", tenths as f64 / 10.0)
    }
}

pub fn whole_days_to_tenths(days: i32) -> AppResult<i32> {
    if days < 0 {
        return Err(AppError::bad_request("Balance cannot be negative"));
    }
    days.checked_mul(10)
        .ok_or_else(|| AppError::bad_request("Balance is too large"))
}

pub fn tracks_balance(leave_type: LeaveRequestType) -> bool {
    matches!(
        leave_type,
        LeaveRequestType::Vacation | LeaveRequestType::SickLeave
    )
}

pub fn inclusive_calendar_days(start: Date, end: Date) -> AppResult<i32> {
    if end < start {
        return Err(AppError::bad_request(
            "End date must be on or after start date",
        ));
    }
    let days = (end - start).whole_days() as i32 + 1;
    Ok(days.max(0))
}

pub fn requested_leave_tenths(
    start: Date,
    end: Date,
    day_portion: LeaveDayPortion,
) -> AppResult<i32> {
    if day_portion == LeaveDayPortion::HalfDay {
        if start != end {
            return Err(AppError::bad_request(
                "Half day is only available for single-day requests",
            ));
        }
        return Ok(5);
    }
    let days = inclusive_calendar_days(start, end)?;
    days.checked_mul(10)
        .ok_or_else(|| AppError::bad_request("Leave span is too long"))
}

pub async fn get_defaults(pool: &PgPool) -> AppResult<(i32, i32)> {
    let row: (i32, i32) = sqlx::query_as(
        "SELECT default_vacation_days, default_sick_days FROM company_settings WHERE id = 1",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(row)
}

pub async fn ensure_employee_balances(pool: &PgPool, employee_id: Uuid) -> AppResult<()> {
    let (vacation_days, sick_days) = get_defaults(pool).await?;
    for (leave_type, tenths) in [
        (LeaveRequestType::Vacation, whole_days_to_tenths(vacation_days)?),
        (LeaveRequestType::SickLeave, whole_days_to_tenths(sick_days)?),
    ] {
        sqlx::query(
            "INSERT INTO employee_leave_balances (employee_id, leave_type, balance_days)
             VALUES ($1, $2, $3)
             ON CONFLICT (employee_id, leave_type) DO NOTHING",
        )
        .bind(employee_id)
        .bind(leave_type)
        .bind(tenths)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    }
    Ok(())
}

pub async fn get_snapshot(pool: &PgPool, employee_id: Uuid) -> AppResult<LeaveBalanceSnapshot> {
    ensure_employee_balances(pool, employee_id).await?;
    let vacation: i32 = sqlx::query_scalar(
        "SELECT balance_days FROM employee_leave_balances
         WHERE employee_id = $1 AND leave_type = 'vacation'",
    )
    .bind(employee_id)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    let sick: i32 = sqlx::query_scalar(
        "SELECT balance_days FROM employee_leave_balances
         WHERE employee_id = $1 AND leave_type = 'sick_leave'",
    )
    .bind(employee_id)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(LeaveBalanceSnapshot {
        vacation_tenths: vacation,
        sick_tenths: sick,
    })
}

pub fn days_input_to_tenths(days: f64) -> AppResult<i32> {
    if days < 0.0 {
        return Err(AppError::bad_request("Balance cannot be negative"));
    }
    let tenths = (days * 10.0).round() as i32;
    if tenths % 5 != 0 {
        return Err(AppError::bad_request(
            "Leave balance must be in whole or half-day increments",
        ));
    }
    Ok(tenths)
}

pub async fn set_balance(
    pool: &PgPool,
    employee_id: Uuid,
    leave_type: LeaveRequestType,
    balance_days: f64,
) -> AppResult<()> {
    if !tracks_balance(leave_type) {
        return Err(AppError::bad_request(
            "Only vacation and sick leave balances can be edited",
        ));
    }
    let tenths = days_input_to_tenths(balance_days)?;
    ensure_employee_balances(pool, employee_id).await?;
    sqlx::query(
        "UPDATE employee_leave_balances
         SET balance_days = $3, updated_at = now()
         WHERE employee_id = $1 AND leave_type = $2",
    )
    .bind(employee_id)
    .bind(leave_type)
    .bind(tenths)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

pub async fn pending_leave_tenths(
    pool: &PgPool,
    employee_id: Uuid,
    leave_type: LeaveRequestType,
) -> AppResult<i32> {
    if !tracks_balance(leave_type) {
        return Ok(0);
    }
    let tenths: i32 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(
            CASE
                WHEN start_date = end_date AND day_portion = 'half_day' THEN 5
                ELSE ((end_date - start_date) + 1) * 10
            END
         ), 0)::INTEGER
         FROM leave_requests
         WHERE employee_id = $1 AND leave_type = $2 AND status = 'pending'",
    )
    .bind(employee_id)
    .bind(leave_type)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(tenths)
}

pub async fn assert_sufficient_balance(
    pool: &PgPool,
    employee_id: Uuid,
    leave_type: LeaveRequestType,
    start: Date,
    end: Date,
    day_portion: LeaveDayPortion,
) -> AppResult<()> {
    if !tracks_balance(leave_type) {
        return Ok(());
    }
    let needed = requested_leave_tenths(start, end, day_portion)?;
    let snapshot = get_snapshot(pool, employee_id).await?;
    let reserved = pending_leave_tenths(pool, employee_id, leave_type).await?;
    let available = match leave_type {
        LeaveRequestType::Vacation => snapshot.vacation_tenths,
        LeaveRequestType::SickLeave => snapshot.sick_tenths,
        _ => return Ok(()),
    };
    let remaining = available - reserved;
    if remaining < needed {
        let need_label = format_leave_balance(needed);
        let have_label = format_leave_balance(available);
        let reserved_label = format_leave_balance(reserved);
        return Err(AppError::bad_request(format!(
            "Insufficient {} balance: need {need_label} day(s), have {have_label} ({reserved_label} reserved by pending requests)",
            leave_type.label().to_lowercase()
        )));
    }
    Ok(())
}

pub async fn deduct_for_approved_request(
    pool: &PgPool,
    employee_id: Uuid,
    leave_type: LeaveRequestType,
    start: Date,
    end: Date,
    day_portion: LeaveDayPortion,
) -> AppResult<()> {
    if !tracks_balance(leave_type) {
        return Ok(());
    }
    let tenths = requested_leave_tenths(start, end, day_portion)?;
    ensure_employee_balances(pool, employee_id).await?;
    let updated = sqlx::query(
        "UPDATE employee_leave_balances
         SET balance_days = balance_days - $3, updated_at = now()
         WHERE employee_id = $1 AND leave_type = $2 AND balance_days >= $3",
    )
    .bind(employee_id)
    .bind(leave_type)
    .bind(tenths)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    if updated.rows_affected() == 0 {
        return Err(AppError::bad_request(
            "Insufficient leave balance to approve this request",
        ));
    }
    Ok(())
}

pub async fn refund_for_approved_request_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    employee_id: Uuid,
    leave_type: LeaveRequestType,
    start: Date,
    end: Date,
    day_portion: LeaveDayPortion,
) -> AppResult<()> {
    if !tracks_balance(leave_type) {
        return Ok(());
    }
    let tenths = requested_leave_tenths(start, end, day_portion)?;
    ensure_employee_balances_in_tx(tx, employee_id).await?;
    sqlx::query(
        "UPDATE employee_leave_balances
         SET balance_days = balance_days + $3, updated_at = now()
         WHERE employee_id = $1 AND leave_type = $2",
    )
    .bind(employee_id)
    .bind(leave_type)
    .bind(tenths)
    .execute(&mut **tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

pub async fn deduct_for_approved_request_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    employee_id: Uuid,
    leave_type: LeaveRequestType,
    start: Date,
    end: Date,
    day_portion: LeaveDayPortion,
) -> AppResult<()> {
    if !tracks_balance(leave_type) {
        return Ok(());
    }
    let tenths = requested_leave_tenths(start, end, day_portion)?;
    ensure_employee_balances_in_tx(tx, employee_id).await?;
    let updated = sqlx::query(
        "UPDATE employee_leave_balances
         SET balance_days = balance_days - $3, updated_at = now()
         WHERE employee_id = $1 AND leave_type = $2 AND balance_days >= $3",
    )
    .bind(employee_id)
    .bind(leave_type)
    .bind(tenths)
    .execute(&mut **tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    if updated.rows_affected() == 0 {
        return Err(AppError::bad_request(
            "Insufficient leave balance to approve this request",
        ));
    }
    Ok(())
}

async fn ensure_employee_balances_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    employee_id: Uuid,
) -> AppResult<()> {
    let row: (i32, i32) = sqlx::query_as(
        "SELECT default_vacation_days, default_sick_days FROM company_settings WHERE id = 1",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    for (leave_type, days) in [
        (LeaveRequestType::Vacation, row.0),
        (LeaveRequestType::SickLeave, row.1),
    ] {
        let tenths = whole_days_to_tenths(days)?;
        sqlx::query(
            "INSERT INTO employee_leave_balances (employee_id, leave_type, balance_days)
             VALUES ($1, $2, $3)
             ON CONFLICT (employee_id, leave_type) DO NOTHING",
        )
        .bind(employee_id)
        .bind(leave_type)
        .bind(tenths)
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::LeaveDayPortion;
    use time::Month;

    #[test]
    fn inclusive_days_counts_endpoints() {
        let start = Date::from_calendar_date(2026, Month::June, 10).unwrap();
        let end = Date::from_calendar_date(2026, Month::June, 12).unwrap();
        assert_eq!(inclusive_calendar_days(start, end).unwrap(), 3);
    }

    #[test]
    fn single_day_request_is_one_day() {
        let d = Date::from_calendar_date(2026, Month::June, 10).unwrap();
        assert_eq!(inclusive_calendar_days(d, d).unwrap(), 1);
    }

    #[test]
    fn half_day_single_date_is_five_tenths() {
        let d = Date::from_calendar_date(2026, Month::June, 10).unwrap();
        assert_eq!(
            requested_leave_tenths(d, d, LeaveDayPortion::HalfDay).unwrap(),
            5
        );
    }

    #[test]
    fn format_balance_shows_half_days() {
        assert_eq!(format_leave_balance(125), "12.5");
        assert_eq!(format_leave_balance(120), "12");
    }
}