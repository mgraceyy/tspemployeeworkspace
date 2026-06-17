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
}
