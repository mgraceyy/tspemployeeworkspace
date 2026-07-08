//! Philippine government benefit deductions (employee share).
//!
//! Uses monthly basic salary from compensation and scales by pay-period factor.
//! Rates follow common 2025–2026 employer practice; review with your accountant before production use.

use crate::models::PayPeriodType;
use crate::services::payroll::compute::{period_salary_factor, round_div};

pub const GOV_CODE_SSS: &str = "SSS";
pub const GOV_CODE_PHIC: &str = "PHIC";
pub const GOV_CODE_HDMF: &str = "HDMF";
pub const GOV_CODE_WHT: &str = "WHT";
pub const LWOP_CODE: &str = "LWOP";

pub const AUTO_NOTE: &str = "Auto-calculated";

#[derive(Debug, Clone, Copy, Default)]
pub struct GovernmentDeductionToggles {
    pub sss: bool,
    pub phic: bool,
    pub hdmf: bool,
    pub wht: bool,
}

impl GovernmentDeductionToggles {
    pub fn any_enabled(self) -> bool {
        self.sss || self.phic || self.hdmf || self.wht
    }

    pub fn is_enabled_for_code(self, code: &str) -> bool {
        match code {
            GOV_CODE_SSS => self.sss,
            GOV_CODE_PHIC => self.phic,
            GOV_CODE_HDMF => self.hdmf,
            GOV_CODE_WHT => self.wht,
            _ => false,
        }
    }
}

pub fn is_government_deduction_code(code: &str) -> bool {
    matches!(
        code,
        GOV_CODE_SSS | GOV_CODE_PHIC | GOV_CODE_HDMF | GOV_CODE_WHT
    )
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GovernmentDeductionAmounts {
    pub sss_cents: i64,
    pub phic_cents: i64,
    pub hdmf_cents: i64,
    pub wht_cents: i64,
}

impl GovernmentDeductionAmounts {
    pub fn for_code(self, code: &str) -> i64 {
        match code {
            GOV_CODE_SSS => self.sss_cents,
            GOV_CODE_PHIC => self.phic_cents,
            GOV_CODE_HDMF => self.hdmf_cents,
            GOV_CODE_WHT => self.wht_cents,
            _ => 0,
        }
    }

    pub fn total_monthly_cents(self) -> i64 {
        self.sss_cents + self.phic_cents + self.hdmf_cents + self.wht_cents
    }
}

/// Monthly employee-share amounts before pay-period scaling.
pub fn monthly_government_deductions_cents(
    monthly_salary_cents: i64,
) -> GovernmentDeductionAmounts {
    if monthly_salary_cents <= 0 {
        return GovernmentDeductionAmounts::default();
    }

    let sss = sss_employee_monthly_cents(monthly_salary_cents);
    let phic = philhealth_employee_monthly_cents(monthly_salary_cents);
    let hdmf = pagibig_employee_monthly_cents(monthly_salary_cents);
    let taxable = (monthly_salary_cents - sss - phic - hdmf).max(0);
    let wht = monthly_withholding_tax_cents(taxable);

    GovernmentDeductionAmounts {
        sss_cents: sss,
        phic_cents: phic,
        hdmf_cents: hdmf,
        wht_cents: wht,
    }
}

/// Amounts owed for a single pay period.
pub fn period_government_deductions_cents(
    monthly_salary_cents: i64,
    pay_period: PayPeriodType,
) -> GovernmentDeductionAmounts {
    let monthly = monthly_government_deductions_cents(monthly_salary_cents);
    let factor = period_salary_factor(pay_period);
    GovernmentDeductionAmounts {
        sss_cents: scale_period_amount(monthly.sss_cents, factor),
        phic_cents: scale_period_amount(monthly.phic_cents, factor),
        hdmf_cents: scale_period_amount(monthly.hdmf_cents, factor),
        wht_cents: scale_period_amount(monthly.wht_cents, factor),
    }
}

fn scale_period_amount(monthly_cents: i64, factor: f64) -> i64 {
    if monthly_cents <= 0 {
        return 0;
    }
    (monthly_cents as f64 * factor).round() as i64
}

/// SSS monthly salary credit (MSC) from basic monthly salary in centavos.
fn sss_monthly_salary_credit_pesos(monthly_salary_cents: i64) -> i64 {
    let salary_pesos = monthly_salary_cents / 100;
    if salary_pesos < 5_250 {
        return 5_000;
    }
    if salary_pesos >= 34_750 {
        return 35_000;
    }
    ((salary_pesos - 4_750) / 500) * 500 + 5_000
}

/// Employee SSS share: 5% of MSC (2025+ contribution schedule).
fn sss_employee_monthly_cents(monthly_salary_cents: i64) -> i64 {
    let msc_pesos = sss_monthly_salary_credit_pesos(monthly_salary_cents);
    round_div(msc_pesos * 100 * 5, 100)
}

/// PhilHealth employee share: 2.5% of monthly basic (floor PHP 10k, ceiling PHP 100k salary).
fn philhealth_employee_monthly_cents(monthly_salary_cents: i64) -> i64 {
    const FLOOR: i64 = 1_000_000;
    const CEILING: i64 = 10_000_000;
    let base = monthly_salary_cents.clamp(FLOOR, CEILING);
    round_div(base * 25, 1_000)
}

/// Pag-IBIG employee share: 1% if salary <= PHP 1,500 else 2%, capped at PHP 5,000 fund salary.
fn pagibig_employee_monthly_cents(monthly_salary_cents: i64) -> i64 {
    const CAP: i64 = 500_000;
    const LOW_THRESHOLD: i64 = 150_000;
    const MAX_EMPLOYEE: i64 = 10_000;
    let fund = monthly_salary_cents.min(CAP);
    let share = if monthly_salary_cents <= LOW_THRESHOLD {
        round_div(fund, 100)
    } else {
        round_div(fund * 2, 100)
    };
    share.min(MAX_EMPLOYEE)
}

/// BIR TRAIN monthly withholding tax on taxable income (centavos).
fn monthly_withholding_tax_cents(taxable_monthly_cents: i64) -> i64 {
    if taxable_monthly_cents <= 0 {
        return 0;
    }
    let pesos = taxable_monthly_cents / 100;
    let tax_pesos = if pesos <= 20_833 {
        0.0
    } else if pesos <= 33_332 {
        (pesos as f64 - 20_833.0) * 0.15
    } else if pesos <= 66_666 {
        1_875.0 + (pesos as f64 - 33_333.0) * 0.20
    } else if pesos <= 166_666 {
        8_541.80 + (pesos as f64 - 66_667.0) * 0.25
    } else if pesos <= 666_666 {
        33_541.80 + (pesos as f64 - 166_667.0) * 0.30
    } else {
        183_541.80 + (pesos as f64 - 666_667.0) * 0.35
    };
    (tax_pesos * 100.0).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    const SALARY_26K: i64 = 2_600_000;

    #[test]
    fn sss_msc_brackets() {
        assert_eq!(sss_monthly_salary_credit_pesos(400_000), 5_000);
        assert_eq!(sss_monthly_salary_credit_pesos(550_000), 5_500);
        assert_eq!(sss_monthly_salary_credit_pesos(2_600_000), 26_000);
        assert_eq!(sss_monthly_salary_credit_pesos(40_000_000), 35_000);
    }

    #[test]
    fn sss_employee_share_is_five_percent_of_msc() {
        assert_eq!(sss_employee_monthly_cents(SALARY_26K), 130_000);
    }

    #[test]
    fn philhealth_uses_two_point_five_percent_with_floor() {
        assert_eq!(philhealth_employee_monthly_cents(500_000), 25_000);
        assert_eq!(philhealth_employee_monthly_cents(SALARY_26K), 65_000);
    }

    #[test]
    fn pagibig_caps_employee_share() {
        assert_eq!(pagibig_employee_monthly_cents(100_000), 1_000);
        assert_eq!(pagibig_employee_monthly_cents(SALARY_26K), 10_000);
        assert_eq!(pagibig_employee_monthly_cents(10_000_000), 10_000);
    }

    #[test]
    fn withholding_tax_zero_for_low_income() {
        assert_eq!(monthly_withholding_tax_cents(1_500_000), 0);
    }

    #[test]
    fn withholding_tax_positive_for_mid_income() {
        let monthly = monthly_government_deductions_cents(SALARY_26K);
        assert!(monthly.wht_cents > 0);
        assert!(monthly.sss_cents > 0);
        assert!(monthly.phic_cents > 0);
        assert!(monthly.hdmf_cents > 0);
    }

    #[test]
    fn semimonthly_period_halves_monthly_deductions() {
        let monthly = monthly_government_deductions_cents(SALARY_26K);
        let semimonthly =
            period_government_deductions_cents(SALARY_26K, PayPeriodType::Semimonthly);
        assert_eq!(semimonthly.sss_cents, monthly.sss_cents / 2);
        assert_eq!(semimonthly.phic_cents, monthly.phic_cents / 2);
    }

    #[test]
    fn toggles_gate_codes() {
        let toggles = GovernmentDeductionToggles {
            sss: true,
            phic: false,
            hdmf: true,
            wht: false,
        };
        assert!(toggles.is_enabled_for_code(GOV_CODE_SSS));
        assert!(!toggles.is_enabled_for_code(GOV_CODE_PHIC));
        assert!(!toggles.is_enabled_for_code("LOAN"));
    }

    #[test]
    fn zero_salary_yields_zero_deductions() {
        let amounts = period_government_deductions_cents(0, PayPeriodType::Monthly);
        assert_eq!(amounts, GovernmentDeductionAmounts::default());
    }

    #[test]
    fn monthly_total_sums_components() {
        let amounts = monthly_government_deductions_cents(SALARY_26K);
        assert_eq!(
            amounts.total_monthly_cents(),
            amounts.sss_cents + amounts.phic_cents + amounts.hdmf_cents + amounts.wht_cents
        );
    }
}
