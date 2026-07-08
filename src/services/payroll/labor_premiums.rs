//! Philippine labor law premium pay (additive on top of base + ordinary OT).
//!
//! - Night differential: +10% of hourly rate for work between 22:00–06:00 (company TZ)
//! - Rest day: +30% hourly on regular minutes, +37% hourly on OT minutes (169% − 132%)
//! - Holiday: +100% hourly on regular minutes, +128% hourly on OT minutes (260% − 132%)
//!
//! Ordinary OT at `ot_rate_percent` (default 132%) remains in `ot_pay_cents`; premiums are separate.

use std::collections::HashSet;

use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use time_tz::{timezones::get_by_name, OffsetDateTimeExt, Tz};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{CompanySettings, OtStatus};
use crate::services::payroll::compute::{hourly_rate_cents, round_div};

/// Extra percent of hourly rate applied to regular (non-OT) minutes.
pub const REST_DAY_REGULAR_EXTRA_PERCENT: i64 = 30;
pub const HOLIDAY_REGULAR_EXTRA_PERCENT: i64 = 100;
/// Extra percent on top of ordinary OT (132%) for premium OT minutes.
pub const REST_DAY_OT_EXTRA_PERCENT: i64 = 37;
pub const HOLIDAY_OT_EXTRA_PERCENT: i64 = 128;
pub const NIGHT_DIFF_EXTRA_PERCENT: i64 = 10;

const NIGHT_START_HOUR: u8 = 22;
const NIGHT_END_HOUR: u8 = 6;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaborPremiumToggles {
    pub rest_day: bool,
    pub holiday: bool,
    pub night_diff: bool,
}

impl LaborPremiumToggles {
    pub fn any_enabled(self) -> bool {
        self.rest_day || self.holiday || self.night_diff
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DayPremiumMinutes {
    pub regular_minutes: i32,
    pub ot_minutes: i32,
    pub night_minutes: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayPremiumInput {
    pub monthly_salary_cents: i64,
    pub clock_in: OffsetDateTime,
    pub clock_out: OffsetDateTime,
    pub approved_ot_minutes: i32,
    pub break_minutes: i32,
    pub ot_threshold_minutes: i32,
    pub is_rest_day: bool,
    pub is_holiday: bool,
    pub toggles: LaborPremiumToggles,
}

pub fn split_work_minutes(
    clock_in: OffsetDateTime,
    clock_out: OffsetDateTime,
    break_minutes: i32,
    ot_threshold_minutes: i32,
    approved_ot_minutes: i32,
) -> DayPremiumMinutes {
    if clock_out <= clock_in {
        return DayPremiumMinutes::default();
    }
    let gross = (clock_out - clock_in).whole_minutes() as i32;
    let net = (gross - break_minutes).max(0);
    let regular = net.min(ot_threshold_minutes);
    let computed_ot = (net - ot_threshold_minutes).max(0);
    let ot = if approved_ot_minutes > 0 {
        approved_ot_minutes.min(computed_ot)
    } else {
        0
    };
    DayPremiumMinutes {
        regular_minutes: regular,
        ot_minutes: ot,
        night_minutes: 0,
    }
}

pub fn is_night_local_hour(hour: u8) -> bool {
    !(NIGHT_END_HOUR..NIGHT_START_HOUR).contains(&hour)
}

pub fn night_minutes_between(clock_in: OffsetDateTime, clock_out: OffsetDateTime, tz: &Tz) -> i32 {
    if clock_out <= clock_in {
        return 0;
    }
    let mut count = 0i32;
    let mut t = clock_in;
    while t < clock_out {
        let local = t.to_timezone(tz);
        if is_night_local_hour(local.hour()) {
            count += 1;
        }
        t += time::Duration::minutes(1);
    }
    count
}

pub fn day_premium_cents(input: DayPremiumInput, tz: &Tz) -> i64 {
    if input.monthly_salary_cents <= 0 {
        return 0;
    }
    let hourly = hourly_rate_cents(input.monthly_salary_cents);
    if hourly <= 0 {
        return 0;
    }

    let mut minutes = split_work_minutes(
        input.clock_in,
        input.clock_out,
        input.break_minutes,
        input.ot_threshold_minutes,
        input.approved_ot_minutes,
    );
    if minutes.regular_minutes == 0 && minutes.ot_minutes == 0 {
        return 0;
    }

    if input.toggles.night_diff {
        minutes.night_minutes = night_minutes_between(input.clock_in, input.clock_out, tz);
    }

    let mut premium = 0i64;

    if input.toggles.night_diff && minutes.night_minutes > 0 {
        premium += round_div(
            hourly * minutes.night_minutes as i64 * NIGHT_DIFF_EXTRA_PERCENT,
            100 * 60,
        );
    }

    if input.toggles.holiday && input.is_holiday {
        if minutes.regular_minutes > 0 {
            premium += round_div(
                hourly * minutes.regular_minutes as i64 * HOLIDAY_REGULAR_EXTRA_PERCENT,
                100 * 60,
            );
        }
        if minutes.ot_minutes > 0 {
            premium += round_div(
                hourly * minutes.ot_minutes as i64 * HOLIDAY_OT_EXTRA_PERCENT,
                100 * 60,
            );
        }
    } else if input.toggles.rest_day && input.is_rest_day {
        if minutes.regular_minutes > 0 {
            premium += round_div(
                hourly * minutes.regular_minutes as i64 * REST_DAY_REGULAR_EXTRA_PERCENT,
                100 * 60,
            );
        }
        if minutes.ot_minutes > 0 {
            premium += round_div(
                hourly * minutes.ot_minutes as i64 * REST_DAY_OT_EXTRA_PERCENT,
                100 * 60,
            );
        }
    }

    premium
}

pub async fn load_labor_premium_toggles(pool: &PgPool) -> AppResult<LaborPremiumToggles> {
    let row: (bool, bool, bool) = sqlx::query_as(
        "SELECT premium_rest_day, premium_holiday, premium_night_diff
         FROM company_settings WHERE id = 1",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(LaborPremiumToggles {
        rest_day: row.0,
        holiday: row.1,
        night_diff: row.2,
    })
}

#[derive(sqlx::FromRow)]
struct EntryRow {
    work_date: Date,
    clock_in: OffsetDateTime,
    clock_out: OffsetDateTime,
    ot_minutes: i32,
    ot_status: OtStatus,
}

pub async fn sum_premium_pay_for_period(
    pool: &PgPool,
    employee_id: Uuid,
    period_start: Date,
    period_end: Date,
    settings: &CompanySettings,
    monthly_salary_cents: i64,
) -> AppResult<i64> {
    let toggles = LaborPremiumToggles {
        rest_day: settings.premium_rest_day,
        holiday: settings.premium_holiday,
        night_diff: settings.premium_night_diff,
    };
    if !toggles.any_enabled() || monthly_salary_cents <= 0 {
        return Ok(0);
    }

    let tz = get_by_name(settings.timezone.trim())
        .ok_or_else(|| AppError::bad_request("Unknown company timezone"))?;

    let entries: Vec<EntryRow> = sqlx::query_as(
        "SELECT work_date, clock_in, clock_out, ot_minutes, ot_status
         FROM time_entries
         WHERE employee_id = $1
           AND work_date BETWEEN $2 AND $3
           AND clock_in IS NOT NULL
           AND clock_out IS NOT NULL",
    )
    .bind(employee_id)
    .bind(period_start)
    .bind(period_end)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if entries.is_empty() {
        return Ok(0);
    }

    let holiday_dates: HashSet<Date> =
        crate::services::holidays::list_holidays_between(pool, period_start, period_end)
            .await?
            .into_iter()
            .map(|holiday| holiday.holiday_date)
            .collect();

    let shift_days: HashSet<i16> =
        sqlx::query_scalar("SELECT day_of_week FROM shift_templates WHERE employee_id = $1")
            .bind(employee_id)
            .fetch_all(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?
            .into_iter()
            .collect();

    let mut total = 0i64;
    for entry in entries {
        let approved_ot = match entry.ot_status {
            OtStatus::Approved => entry.ot_minutes,
            OtStatus::None if entry.ot_minutes <= 0 => 0,
            OtStatus::None if !settings.ot_requires_approval => entry.ot_minutes,
            _ => 0,
        };

        let is_holiday = holiday_dates.contains(&entry.work_date);
        let day_of_week = entry.work_date.weekday().number_days_from_sunday() as i16;
        let is_rest_day = !shift_days.contains(&day_of_week);

        total += day_premium_cents(
            DayPremiumInput {
                monthly_salary_cents,
                clock_in: entry.clock_in,
                clock_out: entry.clock_out,
                approved_ot_minutes: approved_ot,
                break_minutes: settings.break_minutes,
                ot_threshold_minutes: settings.ot_threshold_minutes,
                is_rest_day,
                is_holiday,
                toggles,
            },
            tz,
        );
    }

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    const SALARY: i64 = 2_600_000;

    fn manila() -> &'static Tz {
        get_by_name("Asia/Manila").expect("tz")
    }

    #[test]
    fn night_minutes_count_10pm_to_midnight() {
        let start = datetime!(2026-06-15 22:00:00 +08:00);
        let end = datetime!(2026-06-16 00:00:00 +08:00);
        assert_eq!(night_minutes_between(start, end, manila()), 120);
    }

    #[test]
    fn night_minutes_exclude_daytime() {
        let start = datetime!(2026-06-15 09:00:00 +08:00);
        let end = datetime!(2026-06-15 17:00:00 +08:00);
        assert_eq!(night_minutes_between(start, end, manila()), 0);
    }

    #[test]
    fn holiday_regular_premium_is_double_pay_extra() {
        let start = datetime!(2026-06-15 09:00:00 +08:00);
        let end = datetime!(2026-06-15 18:00:00 +08:00);
        let premium = day_premium_cents(
            DayPremiumInput {
                monthly_salary_cents: SALARY,
                clock_in: start,
                clock_out: end,
                approved_ot_minutes: 0,
                break_minutes: 60,
                ot_threshold_minutes: 480,
                is_rest_day: false,
                is_holiday: true,
                toggles: LaborPremiumToggles {
                    holiday: true,
                    ..Default::default()
                },
            },
            manila(),
        );
        // 8h net × PHP 125/hr × 100% extra = PHP 1,000
        assert_eq!(premium, 100_000);
    }

    #[test]
    fn rest_day_ot_premium_adds_37_percent_on_top_of_ordinary_ot() {
        let start = datetime!(2026-06-15 09:00:00 +08:00);
        let end = datetime!(2026-06-15 20:00:00 +08:00);
        let premium = day_premium_cents(
            DayPremiumInput {
                monthly_salary_cents: SALARY,
                clock_in: start,
                clock_out: end,
                approved_ot_minutes: 120,
                break_minutes: 60,
                ot_threshold_minutes: 480,
                is_rest_day: true,
                is_holiday: false,
                toggles: LaborPremiumToggles {
                    rest_day: true,
                    ..Default::default()
                },
            },
            manila(),
        );
        // 8h regular × 30% + 2h OT × 37% on PHP 125/hr
        assert_eq!(premium, 39_250);
    }

    #[test]
    fn disabled_toggles_yield_zero() {
        let start = datetime!(2026-06-15 09:00:00 +08:00);
        let end = datetime!(2026-06-15 18:00:00 +08:00);
        let premium = day_premium_cents(
            DayPremiumInput {
                monthly_salary_cents: SALARY,
                clock_in: start,
                clock_out: end,
                approved_ot_minutes: 0,
                break_minutes: 60,
                ot_threshold_minutes: 480,
                is_rest_day: true,
                is_holiday: true,
                toggles: LaborPremiumToggles::default(),
            },
            manila(),
        );
        assert_eq!(premium, 0);
    }
}
