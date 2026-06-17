use time::Date;

use crate::error::{AppError, AppResult};
use crate::models::{CompanySettings, PayPeriodType};

#[derive(Debug, Clone)]
pub struct ReportPeriod {
    pub start: Date,
    pub end: Date,
    pub label: String,
}

pub fn current_pay_period(
    today: Date,
    pay_period: PayPeriodType,
    biweekly_anchor: Date,
) -> (Date, Date, String) {
    match pay_period {
        PayPeriodType::Weekly => weekly_period(today),
        PayPeriodType::Biweekly => biweekly_period(today, biweekly_anchor),
        PayPeriodType::Semimonthly => semimonthly_period(today),
        PayPeriodType::Monthly => monthly_period(today),
    }
}

pub fn resolve_report_period(
    settings: &CompanySettings,
    today: Date,
    start: Option<&str>,
    end: Option<&str>,
) -> AppResult<ReportPeriod> {
    match (start, end) {
        (Some(start_str), Some(end_str)) => {
            let start_date =
                crate::services::timezone::parse_date(start_str).map_err(AppError::bad_request)?;
            let end_date =
                crate::services::timezone::parse_date(end_str).map_err(AppError::bad_request)?;
            if end_date < start_date {
                return Err(AppError::bad_request(
                    "End date must be on or after start date",
                ));
            }
            Ok(ReportPeriod {
                start: start_date,
                end: end_date,
                label: period_label_for_range(start_date, end_date),
            })
        }
        (None, None) => {
            let (start, end, label) =
                current_pay_period(today, settings.pay_period, settings.pay_period_anchor);
            Ok(ReportPeriod { start, end, label })
        }
        _ => Err(AppError::bad_request(
            "Both start and end dates are required for a custom range",
        )),
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

pub fn period_label_for_range(start: Date, end: Date) -> String {
    format!("{} to {}", format_short_date(start), format_short_date(end))
}

pub fn assert_canonical_pay_period(
    settings: &CompanySettings,
    start: Date,
    end: Date,
) -> AppResult<()> {
    let (expected_start, expected_end, label) =
        current_pay_period(end, settings.pay_period, settings.pay_period_anchor);
    if start == expected_start && end == expected_end {
        return Ok(());
    }
    Err(AppError::bad_request(format!(
        "Payroll requires a full {} pay period ({}). Close exactly that range in Reports before running payroll.",
        pay_period_label(settings.pay_period),
        label
    )))
}

pub fn resolve_timesheet_period(
    today: Date,
    start: Option<&str>,
    end: Option<&str>,
) -> AppResult<(Date, Date)> {
    match (start, end) {
        (Some(start_str), Some(end_str)) => {
            let start_date =
                crate::services::timezone::parse_date(start_str).map_err(AppError::bad_request)?;
            let end_date =
                crate::services::timezone::parse_date(end_str).map_err(AppError::bad_request)?;
            if end_date < start_date {
                return Err(AppError::bad_request(
                    "End date must be on or after start date",
                ));
            }
            Ok((start_date, end_date))
        }
        (None, None) => Ok((today - time::Duration::days(29), today)),
        _ => Err(AppError::bad_request(
            "Both start and end dates are required for a custom range",
        )),
    }
}

fn weekly_period(today: Date) -> (Date, Date, String) {
    let days_from_sunday = today.weekday().number_days_from_sunday() as i64;
    let start = today - time::Duration::days(days_from_sunday);
    let end = start + time::Duration::days(6);
    let label = period_label_for_range(start, end);
    (start, end, label)
}

fn biweekly_period(today: Date, anchor: Date) -> (Date, Date, String) {
    let days_since_anchor = (today - anchor).whole_days();
    let period_index = days_since_anchor.div_euclid(14);
    let start = anchor + time::Duration::days(period_index * 14);
    let end = start + time::Duration::days(13);
    let label = period_label_for_range(start, end);
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

pub(crate) fn format_short_date(date: Date) -> String {
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
        time::Month::April | time::Month::June | time::Month::September | time::Month::November => {
            30
        }
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    fn anchor() -> Date {
        Date::from_calendar_date(2024, Month::January, 1).unwrap()
    }

    #[test]
    fn semimonthly_first_half() {
        let today = Date::from_calendar_date(2026, Month::June, 10).unwrap();
        let (start, end, label) = current_pay_period(today, PayPeriodType::Semimonthly, anchor());
        assert_eq!(start.day(), 1);
        assert_eq!(end.day(), 15);
        assert!(label.contains("2026-06-01"));
    }

    #[test]
    fn semimonthly_second_half() {
        let today = Date::from_calendar_date(2026, Month::June, 20).unwrap();
        let (start, end, _) = current_pay_period(today, PayPeriodType::Semimonthly, anchor());
        assert_eq!(start.day(), 16);
        assert_eq!(end.day(), 30);
    }

    #[test]
    fn monthly_period() {
        let today = Date::from_calendar_date(2026, Month::February, 14).unwrap();
        let (start, end, _) = current_pay_period(today, PayPeriodType::Monthly, anchor());
        assert_eq!(start.day(), 1);
        assert_eq!(end.day(), 28);
    }

    #[test]
    fn weekly_period_starts_sunday() {
        let today = Date::from_calendar_date(2026, Month::June, 17).unwrap();
        let (start, end, _) = current_pay_period(today, PayPeriodType::Weekly, anchor());
        assert_eq!(start.weekday(), time::Weekday::Sunday);
        assert_eq!(end.weekday(), time::Weekday::Saturday);
        assert_eq!((end - start).whole_days(), 6);
    }

    #[test]
    fn biweekly_period_is_fourteen_days() {
        let today = Date::from_calendar_date(2026, Month::June, 17).unwrap();
        let (start, end, _) = current_pay_period(today, PayPeriodType::Biweekly, anchor());
        assert_eq!((end - start).whole_days(), 13);
        assert!(today >= start && today <= end);
    }

    #[test]
    fn biweekly_period_respects_custom_anchor() {
        let custom_anchor = Date::from_calendar_date(2026, Month::January, 5).unwrap();
        let today = Date::from_calendar_date(2026, Month::January, 20).unwrap();
        let (start, end, _) = current_pay_period(today, PayPeriodType::Biweekly, custom_anchor);
        assert_eq!(
            start,
            Date::from_calendar_date(2026, Month::January, 19).unwrap()
        );
        assert_eq!(
            end,
            Date::from_calendar_date(2026, Month::February, 1).unwrap()
        );
    }

    #[test]
    fn assert_canonical_pay_period_accepts_full_semimonthly_range() {
        let settings = CompanySettings {
            company_name: "Test".into(),
            break_minutes: 60,
            ot_threshold_minutes: 480,
            grace_minutes: 5,
            pay_period: PayPeriodType::Semimonthly,
            pay_period_anchor: anchor(),
            timezone: "Asia/Manila".into(),
            ot_requires_approval: true,
        };
        let start = Date::from_calendar_date(2026, Month::June, 1).unwrap();
        let end = Date::from_calendar_date(2026, Month::June, 15).unwrap();
        assert!(assert_canonical_pay_period(&settings, start, end).is_ok());
    }

    #[test]
    fn assert_canonical_pay_period_rejects_partial_range() {
        let settings = CompanySettings {
            company_name: "Test".into(),
            break_minutes: 60,
            ot_threshold_minutes: 480,
            grace_minutes: 5,
            pay_period: PayPeriodType::Semimonthly,
            pay_period_anchor: anchor(),
            timezone: "Asia/Manila".into(),
            ot_requires_approval: true,
        };
        let start = Date::from_calendar_date(2026, Month::June, 1).unwrap();
        let end = Date::from_calendar_date(2026, Month::June, 10).unwrap();
        assert!(assert_canonical_pay_period(&settings, start, end).is_err());
    }

    #[test]
    fn resolve_custom_date_range() {
        let settings = CompanySettings {
            company_name: "Test".into(),
            break_minutes: 60,
            ot_threshold_minutes: 480,
            grace_minutes: 5,
            pay_period: PayPeriodType::Semimonthly,
            pay_period_anchor: anchor(),
            timezone: "Asia/Manila".into(),
            ot_requires_approval: true,
        };
        let today = Date::from_calendar_date(2026, Month::June, 17).unwrap();
        let period =
            resolve_report_period(&settings, today, Some("2026-06-01"), Some("2026-06-10"))
                .unwrap();
        assert_eq!(period.start.day(), 1);
        assert_eq!(period.end.day(), 10);
    }
}
