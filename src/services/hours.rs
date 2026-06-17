use time::OffsetDateTime;

use crate::models::CompanySettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoursBreakdown {
    pub gross_minutes: i32,
    pub net_minutes: i32,
    pub regular_minutes: i32,
    pub ot_minutes: i32,
}

pub fn calculate(
    clock_in: OffsetDateTime,
    clock_out: OffsetDateTime,
    settings: &CompanySettings,
) -> HoursBreakdown {
    let gross = (clock_out - clock_in).whole_minutes() as i32;
    let net = (gross - settings.break_minutes).max(0);
    let regular = net.min(settings.ot_threshold_minutes);
    let ot = (net - settings.ot_threshold_minutes).max(0);

    HoursBreakdown {
        gross_minutes: gross,
        net_minutes: net,
        regular_minutes: regular,
        ot_minutes: ot,
    }
}

pub fn format_minutes(minutes: i32) -> String {
    let hours = minutes / 60;
    let mins = minutes % 60;
    if mins == 0 {
        format!("{hours}h")
    } else {
        format!("{hours}h {mins}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{settings::CompanySettings, PayPeriodType};

    fn test_settings() -> CompanySettings {
        CompanySettings {
            company_name: "Test".into(),
            break_minutes: 60,
            ot_threshold_minutes: 480,
            grace_minutes: 5,
            pay_period: PayPeriodType::Semimonthly,
            pay_period_anchor: time::Date::from_calendar_date(2024, time::Month::January, 1)
                .unwrap(),
            timezone: "Asia/Manila".into(),
            ot_requires_approval: true,
        }
    }

    #[test]
    fn normal_day_no_overtime() {
        let settings = test_settings();
        let clock_in = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let clock_out = clock_in + time::Duration::hours(9);
        let result = calculate(clock_in, clock_out, &settings);
        assert_eq!(result.gross_minutes, 540);
        assert_eq!(result.net_minutes, 480);
        assert_eq!(result.regular_minutes, 480);
        assert_eq!(result.ot_minutes, 0);
    }

    #[test]
    fn day_with_overtime() {
        let settings = test_settings();
        let clock_in = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let clock_out = clock_in + time::Duration::hours(11);
        let result = calculate(clock_in, clock_out, &settings);
        assert_eq!(result.net_minutes, 600);
        assert_eq!(result.regular_minutes, 480);
        assert_eq!(result.ot_minutes, 120);
    }
}
