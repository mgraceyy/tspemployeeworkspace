use time::{Date, OffsetDateTime, PrimitiveDateTime, Time};
use time_tz::{timezones, OffsetDateTimeExt, OffsetResult, PrimitiveDateTimeExt, Tz};

use crate::error::{AppError, AppResult};
use crate::models::CompanySettings;

pub const DEFAULT_TIMEZONE: &str = "Asia/Manila";

pub fn resolve_timezone(name: &str) -> AppResult<&'static Tz> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::bad_request("Timezone is required"));
    }
    timezones::get_by_name(trimmed)
        .ok_or_else(|| AppError::bad_request(format!("Unknown timezone: {trimmed}")))
}

pub fn validate_timezone(name: &str) -> AppResult<()> {
    resolve_timezone(name).map(|_| ())
}

pub fn now_in_timezone(tz_name: &str) -> AppResult<OffsetDateTime> {
    let tz = resolve_timezone(tz_name)?;
    Ok(OffsetDateTime::now_utc().to_timezone(tz))
}

pub fn date_now_in_timezone(tz_name: &str) -> AppResult<Date> {
    Ok(now_in_timezone(tz_name)?.date())
}

pub fn now_company(settings: &CompanySettings) -> AppResult<OffsetDateTime> {
    now_in_timezone(&settings.timezone)
}

pub fn company_date_now(settings: &CompanySettings) -> AppResult<Date> {
    date_now_in_timezone(&settings.timezone)
}

pub fn combine_date_time(date: Date, time: Time, tz_name: &str) -> AppResult<OffsetDateTime> {
    let tz = resolve_timezone(tz_name)?;
    match PrimitiveDateTime::new(date, time).assume_timezone(tz) {
        OffsetResult::Some(dt) => Ok(dt),
        OffsetResult::Ambiguous(dt, _) => Ok(dt),
        OffsetResult::None => Err(AppError::bad_request(
            "Invalid local date/time for the configured timezone",
        )),
    }
}

fn hour12_parts(hour24: u8) -> (u8, &'static str) {
    match hour24 {
        0 => (12, "AM"),
        1..=11 => (hour24, "AM"),
        12 => (12, "PM"),
        _ => (hour24 - 12, "PM"),
    }
}

pub fn format_time_of_day(time: Time) -> String {
    let (hour12, period) = hour12_parts(time.hour());
    format!("{:02}:{:02} {}", hour12, time.minute(), period)
}

pub fn format_time(dt: OffsetDateTime, tz_name: &str) -> String {
    let tz = resolve_timezone(tz_name).unwrap_or_else(|_| {
        resolve_timezone(DEFAULT_TIMEZONE).expect("default timezone must exist")
    });
    let local = dt.to_timezone(tz);
    format_time_of_day(local.time())
}

pub fn format_date(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

pub fn format_time_input(dt: OffsetDateTime, tz_name: &str) -> String {
    let tz = resolve_timezone(tz_name).unwrap_or_else(|_| {
        resolve_timezone(DEFAULT_TIMEZONE).expect("default timezone must exist")
    });
    let local = dt.to_timezone(tz);
    format!("{:02}:{:02}", local.hour(), local.minute())
}

pub fn parse_date(value: &str) -> Result<Date, &'static str> {
    let trimmed = value.trim();
    let parts: Vec<_> = trimmed.split('-').collect();
    if parts.len() != 3 {
        return Err("Date must be YYYY-MM-DD");
    }
    let year: i32 = parts[0].parse().map_err(|_| "Invalid year")?;
    let month: u8 = parts[1].parse().map_err(|_| "Invalid month")?;
    let day: u8 = parts[2].parse().map_err(|_| "Invalid day")?;
    let month = time::Month::try_from(month).map_err(|_| "Invalid month")?;
    Date::from_calendar_date(year, month, day).map_err(|_| "Invalid date")
}

pub fn parse_time_on_date(
    date: Date,
    value: &str,
    tz_name: &str,
) -> Result<OffsetDateTime, &'static str> {
    let trimmed = value.trim();
    let parts: Vec<_> = trimmed.split(':').collect();
    if parts.len() != 2 {
        return Err("Time must be HH:MM");
    }
    let hour: u8 = parts[0].parse().map_err(|_| "Invalid hour")?;
    let minute: u8 = parts[1].parse().map_err(|_| "Invalid minute")?;
    let time = Time::from_hms(hour, minute, 0).map_err(|_| "Invalid time")?;
    combine_date_time(date, time, tz_name).map_err(|_| "Invalid timezone")
}

/// Backward-compatible helpers for tests and legacy call sites (defaults to Asia/Manila).
pub fn now_manila() -> OffsetDateTime {
    now_in_timezone(DEFAULT_TIMEZONE).expect("default timezone")
}

pub fn manila_date_now() -> Date {
    date_now_in_timezone(DEFAULT_TIMEZONE).expect("default timezone")
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    #[test]
    fn resolves_common_iana_timezones() {
        assert!(resolve_timezone("Asia/Manila").is_ok());
        assert!(resolve_timezone("America/New_York").is_ok());
        assert!(resolve_timezone("Invalid/Zone").is_err());
    }

    #[test]
    fn format_time_uses_twelve_hour_clock() {
        let date = Date::from_calendar_date(2024, Month::January, 15).unwrap();
        assert_eq!(
            format_time_of_day(Time::from_hms(0, 30, 0).unwrap()),
            "12:30 AM"
        );
        assert_eq!(
            format_time_of_day(Time::from_hms(9, 5, 0).unwrap()),
            "09:05 AM"
        );
        assert_eq!(
            format_time_of_day(Time::from_hms(12, 0, 0).unwrap()),
            "12:00 PM"
        );
        assert_eq!(
            format_time_of_day(Time::from_hms(17, 45, 0).unwrap()),
            "05:45 PM"
        );
        let morning = combine_date_time(date, Time::from_hms(9, 15, 0).unwrap(), "Asia/Manila")
            .unwrap();
        assert!(format_time(morning, "Asia/Manila").ends_with("AM"));
    }

    #[test]
    fn combine_date_time_respects_timezone_offset() {
        let date = Date::from_calendar_date(2024, Month::January, 15).unwrap();
        let time = Time::from_hms(9, 0, 0).unwrap();
        let manila = combine_date_time(date, time, "Asia/Manila").unwrap();
        let utc = combine_date_time(date, time, "UTC").unwrap();
        assert_eq!((utc - manila).whole_hours(), 8);
    }
}
