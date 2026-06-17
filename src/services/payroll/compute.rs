//! Monthly payroll computation for TalaSora Prime policy (see docs/PAYROLL.md).
//!
//! - All employees: monthly salary
//! - OT: hourly equivalent × OT rate (default 132%)
//! - No-shows: reduce pay by one daily rate per day
//! - Sick/vacation/official/offset leave: informational only (no pay adjustment)

use crate::models::PayPeriodType;

/// Standard working days per month (Philippine payroll convention).
pub const MONTHLY_WORKING_DAYS: i64 = 26;

/// Standard paid minutes per day (8h); matches default `ot_threshold_minutes`.
pub const STANDARD_DAILY_MINUTES: i64 = 480;

#[derive(Debug, Clone, Copy)]
pub struct GrossPayInput {
    pub monthly_salary_cents: i64,
    pub ot_rate_percent: i32,
    pub pay_period: PayPeriodType,
    pub approved_ot_minutes: i64,
    pub no_show_days: i64,
}

pub fn round_div(numerator: i64, denominator: i64) -> i64 {
    if denominator == 0 {
        return 0;
    }
    (numerator + denominator / 2) / denominator
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

pub fn no_show_deduction_cents(monthly_salary_cents: i64, no_show_days: i64) -> i64 {
    if no_show_days <= 0 {
        return 0;
    }
    daily_rate_cents(monthly_salary_cents) * no_show_days
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

pub fn gross_pay_cents(input: &GrossPayInput) -> i64 {
    let base = base_pay_cents_for_period(input.monthly_salary_cents, input.pay_period);
    let deduction = no_show_deduction_cents(input.monthly_salary_cents, input.no_show_days);
    let ot = ot_pay_cents(
        input.monthly_salary_cents,
        input.approved_ot_minutes,
        input.ot_rate_percent,
    );
    (base - deduction + ot).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            ot_rate_percent: 132,
            pay_period: PayPeriodType::Semimonthly,
            approved_ot_minutes: 0,
            no_show_days: 2,
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
            ot_rate_percent: 132,
            pay_period: PayPeriodType::Semimonthly,
            approved_ot_minutes: 60,
            no_show_days: 1,
        };
        // base 13,000 - 1,000 + OT(1h)=125×1.32=165 → 12,165
        assert_eq!(gross_pay_cents(&input), 1_216_500);
    }

    #[test]
    fn leave_days_do_not_affect_gross() {
        let with_leave = GrossPayInput {
            monthly_salary_cents: SALARY,
            ot_rate_percent: 132,
            pay_period: PayPeriodType::Monthly,
            approved_ot_minutes: 0,
            no_show_days: 0,
        };
        assert_eq!(gross_pay_cents(&with_leave), SALARY);
    }
}
