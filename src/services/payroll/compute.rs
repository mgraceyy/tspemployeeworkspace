//! Monthly payroll computation for TalaSora Prime policy (see docs/PAYROLL.md).
//!
//! - All employees: monthly salary
//! - OT: hourly equivalent × OT rate (default 132%)
//! - No-shows: reduce pay by one daily rate per day
//! - LWOP: unpaid leave deducted as a payroll line item (auto on draft runs)
//! - Sick/vacation/official/offset leave: informational only (no pay adjustment)
//! - Hire / separation dates: prorate period base and allowances by calendar days employed

use time::Date;

use crate::models::PayPeriodType;

/// Standard working days per month (Philippine payroll convention).
pub const MONTHLY_WORKING_DAYS: i64 = 26;

/// Standard paid minutes per day (8h); matches default `ot_threshold_minutes`.
pub const STANDARD_DAILY_MINUTES: i64 = 480;

#[derive(Debug, Clone, Copy)]
pub struct EmploymentSpan {
    pub date_hired: Option<Date>,
    pub date_separated: Option<Date>,
}

#[derive(Debug, Clone, Copy)]
pub struct GrossPayInput {
    pub monthly_salary_cents: i64,
    pub monthly_allowance_cents: i64,
    pub ot_rate_percent: i32,
    pub pay_period: PayPeriodType,
    pub approved_ot_minutes: i64,
    pub no_show_days: i64,
    pub premium_pay_cents: i64,
    pub employed_days: Option<i64>,
    pub period_calendar_days: Option<i64>,
}

pub fn round_div(numerator: i64, denominator: i64) -> i64 {
    if denominator == 0 {
        return 0;
    }
    (numerator + denominator / 2) / denominator
}

pub fn inclusive_calendar_days(start: Date, end: Date) -> i64 {
    if end < start {
        return 0;
    }
    (end - start).whole_days() + 1
}

pub fn employed_days_in_period(
    period_start: Date,
    period_end: Date,
    span: EmploymentSpan,
) -> (i64, i64) {
    let period_days = inclusive_calendar_days(period_start, period_end);
    let emp_start = span.date_hired.unwrap_or(period_start);
    let emp_start = emp_start.max(period_start);
    let emp_end = span.date_separated.unwrap_or(period_end);
    let emp_end = emp_end.min(period_end);
    if emp_end < emp_start {
        (0, period_days)
    } else {
        (inclusive_calendar_days(emp_start, emp_end), period_days)
    }
}

pub fn daily_rate_cents(monthly_salary_cents: i64) -> i64 {
    round_div(monthly_salary_cents, MONTHLY_WORKING_DAYS)
}

pub fn hourly_rate_cents(monthly_salary_cents: i64) -> i64 {
    round_div(
        daily_rate_cents(monthly_salary_cents) * 60,
        STANDARD_DAILY_MINUTES,
    )
}

/// Fraction of monthly salary owed for this pay period type.
pub fn period_salary_factor(pay_period: PayPeriodType) -> f64 {
    match pay_period {
        PayPeriodType::Weekly => 12.0 / 52.0,
        PayPeriodType::Biweekly => 1.0 / 2.0,
        PayPeriodType::Semimonthly => 1.0 / 2.0,
        PayPeriodType::Monthly => 1.0,
    }
}

pub fn base_pay_cents_for_period(monthly_salary_cents: i64, pay_period: PayPeriodType) -> i64 {
    let factor = period_salary_factor(pay_period);
    (monthly_salary_cents as f64 * factor).round() as i64
}

pub fn prorated_period_amount_cents(
    full_period_cents: i64,
    employed_days: i64,
    period_calendar_days: i64,
) -> i64 {
    if period_calendar_days <= 0 || employed_days >= period_calendar_days {
        return full_period_cents;
    }
    if employed_days <= 0 {
        return 0;
    }
    round_div(full_period_cents * employed_days, period_calendar_days)
}

pub fn no_show_deduction_cents(monthly_salary_cents: i64, no_show_days: i64) -> i64 {
    if no_show_days <= 0 {
        return 0;
    }
    daily_rate_cents(monthly_salary_cents) * no_show_days
}

pub fn lwop_deduction_cents(monthly_salary_cents: i64, lwop_days: i64) -> i64 {
    no_show_deduction_cents(monthly_salary_cents, lwop_days)
}

pub fn ot_pay_cents(
    monthly_salary_cents: i64,
    approved_ot_minutes: i64,
    ot_rate_percent: i32,
) -> i64 {
    if approved_ot_minutes <= 0 {
        return 0;
    }
    let hourly = hourly_rate_cents(monthly_salary_cents);
    round_div(
        hourly * approved_ot_minutes * ot_rate_percent as i64,
        100 * 60,
    )
}

pub fn allowance_pay_cents_for_period(
    monthly_allowance_cents: i64,
    pay_period: PayPeriodType,
) -> i64 {
    base_pay_cents_for_period(monthly_allowance_cents, pay_period)
}

fn prorate_if_needed(
    full_period_cents: i64,
    employed_days: Option<i64>,
    period_calendar_days: Option<i64>,
) -> i64 {
    match (employed_days, period_calendar_days) {
        (Some(employed), Some(period_days)) => {
            prorated_period_amount_cents(full_period_cents, employed, period_days)
        }
        _ => full_period_cents,
    }
}

pub fn gross_pay_cents(input: &GrossPayInput) -> i64 {
    let full_base = base_pay_cents_for_period(input.monthly_salary_cents, input.pay_period);
    let base = prorate_if_needed(full_base, input.employed_days, input.period_calendar_days);
    let full_allowance =
        allowance_pay_cents_for_period(input.monthly_allowance_cents, input.pay_period);
    let allowance = prorate_if_needed(
        full_allowance,
        input.employed_days,
        input.period_calendar_days,
    );
    let deduction = no_show_deduction_cents(input.monthly_salary_cents, input.no_show_days);
    let ot = ot_pay_cents(
        input.monthly_salary_cents,
        input.approved_ot_minutes,
        input.ot_rate_percent,
    );
    (base + allowance - deduction + ot + input.premium_pay_cents).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    const SALARY: i64 = 2_600_000; // PHP 26,000.00 — daily rate = 1,000.00

    #[test]
    fn daily_and_hourly_rates() {
        assert_eq!(daily_rate_cents(SALARY), 100_000); // PHP 1,000/day
        assert_eq!(hourly_rate_cents(SALARY), 12_500); // PHP 125/hr
    }

    #[test]
    fn semimonthly_base_is_half_monthly() {
        assert_eq!(
            base_pay_cents_for_period(SALARY, PayPeriodType::Semimonthly),
            1_300_000
        );
    }

    #[test]
    fn no_show_reduces_gross() {
        let input = GrossPayInput {
            monthly_salary_cents: SALARY,
            monthly_allowance_cents: 0,
            ot_rate_percent: 132,
            pay_period: PayPeriodType::Semimonthly,
            approved_ot_minutes: 0,
            no_show_days: 2,
            premium_pay_cents: 0,
            employed_days: None,
            period_calendar_days: None,
        };
        // 13,000 - 2×1,000 = 11,000
        assert_eq!(gross_pay_cents(&input), 1_100_000);
    }

    #[test]
    fn ot_at_132_percent() {
        let ot = ot_pay_cents(SALARY, 120, 132); // 2 hours
                                                 // 125.00 × 2 × 1.32 = 330.00
        assert_eq!(ot, 33_000);
    }

    #[test]
    fn full_gross_with_ot_and_no_show() {
        let input = GrossPayInput {
            monthly_salary_cents: SALARY,
            monthly_allowance_cents: 0,
            ot_rate_percent: 132,
            pay_period: PayPeriodType::Semimonthly,
            approved_ot_minutes: 60,
            no_show_days: 1,
            premium_pay_cents: 0,
            employed_days: None,
            period_calendar_days: None,
        };
        // base 13,000 - 1,000 + OT(1h)=125×1.32=165 → 12,165
        assert_eq!(gross_pay_cents(&input), 1_216_500);
    }

    #[test]
    fn weekly_base_uses_twelve_over_fifty_two_factor() {
        assert_eq!(
            base_pay_cents_for_period(SALARY, PayPeriodType::Weekly),
            600_000
        );
    }

    #[test]
    fn biweekly_base_is_half_monthly() {
        assert_eq!(
            base_pay_cents_for_period(SALARY, PayPeriodType::Biweekly),
            1_300_000
        );
    }

    #[test]
    fn semimonthly_allowance_is_half_monthly() {
        assert_eq!(
            allowance_pay_cents_for_period(150_000, PayPeriodType::Semimonthly),
            75_000
        );
    }

    #[test]
    fn allowances_increase_gross_pay() {
        let input = GrossPayInput {
            monthly_salary_cents: SALARY,
            monthly_allowance_cents: 150_000,
            ot_rate_percent: 132,
            pay_period: PayPeriodType::Semimonthly,
            approved_ot_minutes: 0,
            no_show_days: 0,
            premium_pay_cents: 0,
            employed_days: None,
            period_calendar_days: None,
        };
        // base 13,000 + allowance 750 = 13,750
        assert_eq!(gross_pay_cents(&input), 1_375_000);
    }

    #[test]
    fn leave_days_do_not_affect_gross() {
        let with_leave = GrossPayInput {
            monthly_salary_cents: SALARY,
            monthly_allowance_cents: 0,
            ot_rate_percent: 132,
            pay_period: PayPeriodType::Monthly,
            approved_ot_minutes: 0,
            no_show_days: 0,
            premium_pay_cents: 0,
            employed_days: None,
            period_calendar_days: None,
        };
        assert_eq!(gross_pay_cents(&with_leave), SALARY);
    }

    #[test]
    fn premium_pay_increases_gross() {
        let input = GrossPayInput {
            monthly_salary_cents: SALARY,
            monthly_allowance_cents: 0,
            ot_rate_percent: 132,
            pay_period: PayPeriodType::Semimonthly,
            approved_ot_minutes: 0,
            no_show_days: 0,
            premium_pay_cents: 50_000,
            employed_days: None,
            period_calendar_days: None,
        };
        assert_eq!(gross_pay_cents(&input), 1_350_000);
    }

    #[test]
    fn prorates_semimonthly_base_for_mid_period_hire() {
        let period_start = Date::from_calendar_date(2026, Month::June, 1).unwrap();
        let period_end = Date::from_calendar_date(2026, Month::June, 15).unwrap();
        let hired = Date::from_calendar_date(2026, Month::June, 10).unwrap();
        let (employed, period_days) = employed_days_in_period(
            period_start,
            period_end,
            EmploymentSpan {
                date_hired: Some(hired),
                date_separated: None,
            },
        );
        assert_eq!(period_days, 15);
        assert_eq!(employed, 6);

        let input = GrossPayInput {
            monthly_salary_cents: SALARY,
            monthly_allowance_cents: 0,
            ot_rate_percent: 132,
            pay_period: PayPeriodType::Semimonthly,
            approved_ot_minutes: 0,
            no_show_days: 0,
            premium_pay_cents: 0,
            employed_days: Some(employed),
            period_calendar_days: Some(period_days),
        };
        // 13,000 × 6/15 = 5,200
        assert_eq!(gross_pay_cents(&input), 520_000);
    }

    #[test]
    fn prorates_for_mid_period_separation() {
        let period_start = Date::from_calendar_date(2026, Month::June, 16).unwrap();
        let period_end = Date::from_calendar_date(2026, Month::June, 30).unwrap();
        let separated = Date::from_calendar_date(2026, Month::June, 20).unwrap();
        let (employed, period_days) = employed_days_in_period(
            period_start,
            period_end,
            EmploymentSpan {
                date_hired: None,
                date_separated: Some(separated),
            },
        );
        assert_eq!(period_days, 15);
        assert_eq!(employed, 5);

        let input = GrossPayInput {
            monthly_salary_cents: SALARY,
            monthly_allowance_cents: 0,
            ot_rate_percent: 132,
            pay_period: PayPeriodType::Semimonthly,
            approved_ot_minutes: 0,
            no_show_days: 0,
            premium_pay_cents: 0,
            employed_days: Some(employed),
            period_calendar_days: Some(period_days),
        };
        // 13,000 × 5/15 ≈ 4,333
        assert_eq!(gross_pay_cents(&input), 433_333);
    }

    #[test]
    fn lwop_deduction_matches_daily_rate() {
        assert_eq!(lwop_deduction_cents(SALARY, 2), 200_000);
    }
}
