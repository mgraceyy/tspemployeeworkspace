use time::{Date, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

pub const MANILA_OFFSET: UtcOffset = match UtcOffset::from_hms(8, 0, 0) {
    Ok(offset) => offset,
    Err(_) => panic!("invalid Manila offset"),
};

pub fn now_manila() -> OffsetDateTime {
    OffsetDateTime::now_utc().to_offset(MANILA_OFFSET)
}

pub fn manila_date_now() -> Date {
    now_manila().date()
}

pub fn combine_date_time(date: Date, time: Time) -> OffsetDateTime {
    PrimitiveDateTime::new(date, time)
        .assume_offset(MANILA_OFFSET)
}

pub fn format_time(dt: OffsetDateTime) -> String {
    let local = dt.to_offset(MANILA_OFFSET);
    format!(
        "{:02}:{:02} {}",
        local.hour(),
        local.minute(),
        if local.hour() < 12 { "AM" } else { "PM" }
    )
}

pub fn format_date(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

pub fn format_time_input(dt: OffsetDateTime) -> String {
    let local = dt.to_offset(MANILA_OFFSET);
    format!("{:02}:{:02}", local.hour(), local.minute())
}

pub fn parse_time_on_date(date: Date, value: &str) -> Result<OffsetDateTime, &'static str> {
    let trimmed = value.trim();
    let parts: Vec<_> = trimmed.split(':').collect();
    if parts.len() != 2 {
        return Err("Time must be HH:MM");
    }
    let hour: u8 = parts[0].parse().map_err(|_| "Invalid hour")?;
    let minute: u8 = parts[1].parse().map_err(|_| "Invalid minute")?;
    let time = Time::from_hms(hour, minute, 0).map_err(|_| "Invalid time")?;
    Ok(combine_date_time(date, time))
}