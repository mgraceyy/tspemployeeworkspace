use serde::{Deserialize, Serialize};
use sqlx::Type;
use time::Date;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[sqlx(type_name = "pay_period_type", rename_all = "snake_case")]
pub enum PayPeriodType {
    Weekly,
    Biweekly,
    Semimonthly,
    Monthly,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CompanySettings {
    pub company_name: String,
    pub break_minutes: i32,
    pub ot_threshold_minutes: i32,
    pub grace_minutes: i32,
    pub pay_period: PayPeriodType,
    pub pay_period_anchor: Date,
    pub timezone: String,
    pub ot_requires_approval: bool,
    pub journal_salary_expense_account: String,
    pub journal_net_payable_account: String,
    pub journal_salary_expense_label: String,
    pub journal_net_payable_label: String,
    pub auto_deduct_sss: bool,
    pub auto_deduct_phic: bool,
    pub auto_deduct_hdmf: bool,
    pub auto_deduct_wht: bool,
    pub premium_rest_day: bool,
    pub premium_holiday: bool,
    pub premium_night_diff: bool,
    pub default_vacation_days: i32,
    pub default_sick_days: i32,
}

impl Default for CompanySettings {
    fn default() -> Self {
        Self {
            company_name: String::new(),
            break_minutes: 60,
            ot_threshold_minutes: 480,
            grace_minutes: 5,
            pay_period: PayPeriodType::Semimonthly,
            pay_period_anchor: Date::from_calendar_date(2024, time::Month::January, 1).unwrap(),
            timezone: "Asia/Manila".into(),
            ot_requires_approval: true,
            journal_salary_expense_account: "5100".into(),
            journal_net_payable_account: "2100".into(),
            journal_salary_expense_label: "Salaries expense".into(),
            journal_net_payable_label: "Net pay payable".into(),
            auto_deduct_sss: false,
            auto_deduct_phic: false,
            auto_deduct_hdmf: false,
            auto_deduct_wht: false,
            premium_rest_day: false,
            premium_holiday: false,
            premium_night_diff: false,
            default_vacation_days: 0,
            default_sick_days: 0,
        }
    }
}

impl CompanySettings {
    pub fn government_deduction_toggles(&self) -> crate::services::payroll::GovernmentDeductionToggles {
        crate::services::payroll::GovernmentDeductionToggles {
            sss: self.auto_deduct_sss,
            phic: self.auto_deduct_phic,
            hdmf: self.auto_deduct_hdmf,
            wht: self.auto_deduct_wht,
        }
    }

    pub fn labor_premium_toggles(&self) -> crate::services::payroll::LaborPremiumToggles {
        crate::services::payroll::LaborPremiumToggles {
            rest_day: self.premium_rest_day,
            holiday: self.premium_holiday,
            night_diff: self.premium_night_diff,
        }
    }
}