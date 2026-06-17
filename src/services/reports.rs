use sqlx::PgPool;
use time::Date;

use crate::error::{AppError, AppResult};
use crate::models::{OtStatus, PayPeriodType};

#[derive(Debug, sqlx::FromRow)]
pub struct PayrollRow {
    pub employee_code: String,
    pub full_name: String,
    pub regular_minutes: i64,
    pub approved_ot_minutes: i64,
    pub pending_ot_minutes: i64,
}

const BIWEEKLY_ANCHOR: Date = match Date::from_calendar_date(2024, time::Month::January, 1) {
    Ok(d) => d,
    Err(_) => panic!("invalid biweekly anchor date"),
};

pub fn current_pay_period(today: Date, pay_period: PayPeriodType) -> (Date, Date, String) {
    match pay_period {
        PayPeriodType::Weekly => weekly_period(today),
        PayPeriodType::Biweekly => biweekly_period(today),
        PayPeriodType::Semimonthly => semimonthly_period(today),
        PayPeriodType::Monthly => monthly_period(today),
    }
}

pub fn pay_period_label(pay_period: PayPeriodType) -> &'static str {
    match pay_period {
        PayPeriodType::Weekly => "Weekly",
        PayPeriodType::Biweekly => "Biweekly",
        PayPeriodType::Semimonthly => "Semi-monthly",
        PayPeriodType::Monthly => "Monthly",
    }
}

fn weekly_period(today: Date) -> (Date, Date, String) {
    let days_from_sunday = today.weekday().number_days_from_sunday() as i64;
    let start = today - time::Duration::days(days_from_sunday);
    let end = start + time::Duration::days(6);
    let label = format!("{} to {}", format_short_date(start), format_short_date(end));
    (start, end, label)
}

fn biweekly_period(today: Date) -> (Date, Date, String) {
    let days_since_anchor = (today - BIWEEKLY_ANCHOR).whole_days();
    let period_index = days_since_anchor.div_euclid(14);
    let start = BIWEEKLY_ANCHOR + time::Duration::days(period_index * 14);
    let end = start + time::Duration::days(13);
    let label = format!("{} to {}", format_short_date(start), format_short_date(end));
    (start, end, label)
}

fn semimonthly_period(today: Date) -> (Date, Date, String) {
    let year = today.year();
    let month = today.month();
    let day = today.day();

    if day <= 15 {
        let start = Date::from_calendar_date(year, month, 1).expect("valid start date");
        let end = Date::from_calendar_date(year, month, 15).expect("valid end date");
        let month_num = u8::from(month);
        let label = format!("{year}-{month_num:02}-01 to {year}-{month_num:02}-15");
        (start, end, label)
    } else {
        let start = Date::from_calendar_date(year, month, 16).expect("valid start date");
        let last_day = last_day_of_month(year, month);
        let end = Date::from_calendar_date(year, month, last_day).expect("valid end date");
        let month_num = u8::from(month);
        let label = format!("{year}-{month_num:02}-16 to {year}-{month_num:02}-{last_day}");
        (start, end, label)
    }
}

fn monthly_period(today: Date) -> (Date, Date, String) {
    let year = today.year();
    let month = today.month();
    let start = Date::from_calendar_date(year, month, 1).expect("valid start date");
    let last_day = last_day_of_month(year, month);
    let end = Date::from_calendar_date(year, month, last_day).expect("valid end date");
    let month_num = u8::from(month);
    let label = format!("{year}-{month_num:02}-01 to {year}-{month_num:02}-{last_day}");
    (start, end, label)
}

fn format_short_date(date: Date) -> String {
    format!(
        "{}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

fn last_day_of_month(year: i32, month: time::Month) -> u8 {
    match month {
        time::Month::February => {
            if time::util::is_leap_year(year) {
                29
            } else {
                28
            }
        }
        time::Month::April
        | time::Month::June
        | time::Month::September
        | time::Month::November => 30,
        _ => 31,
    }
}

pub async fn payroll_summary(
    pool: &PgPool,
    start: Date,
    end: Date,
) -> AppResult<Vec<PayrollRow>> {
    let rows = sqlx::query_as::<_, PayrollRow>(
        "SELECT e.employee_code,
                e.full_name,
                COALESCE(SUM(te.regular_minutes), 0) AS regular_minutes,
                COALESCE(SUM(CASE WHEN te.ot_status = 'approved' THEN te.ot_minutes ELSE 0 END), 0) AS approved_ot_minutes,
                COALESCE(SUM(CASE WHEN te.ot_status = 'pending' THEN te.ot_minutes ELSE 0 END), 0) AS pending_ot_minutes
         FROM employees e
         LEFT JOIN time_entries te
           ON te.employee_id = e.id
          AND te.work_date BETWEEN $1 AND $2
         WHERE e.is_active = TRUE
         GROUP BY e.id, e.employee_code, e.full_name
         ORDER BY e.full_name",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(rows)
}

pub fn payable_minutes(row: &PayrollRow) -> i64 {
    row.regular_minutes + row.approved_ot_minutes
}

pub fn minutes_to_hours_decimal(minutes: i64) -> f64 {
    (minutes as f64) / 60.0
}

pub fn ot_status_payable(status: OtStatus) -> bool {
    status == OtStatus::Approved
}

pub fn build_payroll_xlsx(
    company_name: &str,
    period_label: &str,
    rows: &[PayrollRow],
) -> AppResult<Vec<u8>> {
    use rust_xlsxwriter::{Format, Workbook};

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet
        .set_name("Payroll")
        .map_err(|e| AppError::Internal(e.into()))?;

    let title_format = Format::new().set_bold();
    worksheet
        .write_string_with_format(0, 0, company_name, &title_format)
        .map_err(|e| AppError::Internal(e.into()))?;
    worksheet
        .write_string(1, 0, &format!("Pay period: {period_label}"))
        .map_err(|e| AppError::Internal(e.into()))?;

    let headers = [
        "Employee Code",
        "Name",
        "Regular Hours",
        "Approved OT Hours",
        "Pending OT Hours",
        "Payable Hours",
    ];
    for (col, header) in headers.iter().enumerate() {
        worksheet
            .write_string_with_format(3, col as u16, *header, &title_format)
            .map_err(|e| AppError::Internal(e.into()))?;
    }

    for (i, row) in rows.iter().enumerate() {
        let r = (4 + i) as u32;
        let payable = minutes_to_hours_decimal(payable_minutes(row));
        worksheet
            .write_string(r, 0, &row.employee_code)
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_string(r, 1, &row.full_name)
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_number(r, 2, minutes_to_hours_decimal(row.regular_minutes))
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_number(r, 3, minutes_to_hours_decimal(row.approved_ot_minutes))
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_number(r, 4, minutes_to_hours_decimal(row.pending_ot_minutes))
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_number(r, 5, payable)
            .map_err(|e| AppError::Internal(e.into()))?;
    }

    workbook
        .save_to_buffer()
        .map_err(|e| AppError::Internal(e.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    #[test]
    fn semimonthly_first_half() {
        let today = Date::from_calendar_date(2026, Month::June, 10).unwrap();
        let (start, end, label) = current_pay_period(today, PayPeriodType::Semimonthly);
        assert_eq!(start.day(), 1);
        assert_eq!(end.day(), 15);
        assert!(label.contains("2026-06-01"));
    }

    #[test]
    fn semimonthly_second_half() {
        let today = Date::from_calendar_date(2026, Month::June, 20).unwrap();
        let (start, end, _) = current_pay_period(today, PayPeriodType::Semimonthly);
        assert_eq!(start.day(), 16);
        assert_eq!(end.day(), 30);
    }

    #[test]
    fn monthly_period() {
        let today = Date::from_calendar_date(2026, Month::February, 14).unwrap();
        let (start, end, _) = current_pay_period(today, PayPeriodType::Monthly);
        assert_eq!(start.day(), 1);
        assert_eq!(end.day(), 28);
    }

    #[test]
    fn weekly_period_starts_sunday() {
        let today = Date::from_calendar_date(2026, Month::June, 17).unwrap(); // Wednesday
        let (start, end, _) = current_pay_period(today, PayPeriodType::Weekly);
        assert_eq!(start.weekday(), time::Weekday::Sunday);
        assert_eq!(end.weekday(), time::Weekday::Saturday);
        assert_eq!((end - start).whole_days(), 6);
    }

    #[test]
    fn biweekly_period_is_fourteen_days() {
        let today = Date::from_calendar_date(2026, Month::June, 17).unwrap();
        let (start, end, _) = current_pay_period(today, PayPeriodType::Biweekly);
        assert_eq!((end - start).whole_days(), 13);
        assert!(today >= start && today <= end);
    }
}