use time::{Date, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CompensationProfile {
    pub employee_id: Uuid,
    pub monthly_salary_cents: i64,
    pub ot_rate_percent: i32,
    pub transport_allowance_cents: i64,
    pub meal_allowance_cents: i64,
    pub effective_from: Date,
}

impl CompensationProfile {
    pub fn monthly_allowance_cents(&self) -> i64 {
        self.transport_allowance_cents + self.meal_allowance_cents
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CompensationHistoryRow {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub monthly_salary_cents: i64,
    pub ot_rate_percent: i32,
    pub transport_allowance_cents: i64,
    pub meal_allowance_cents: i64,
    pub effective_from: Date,
    pub effective_to: Option<Date>,
    pub created_at: OffsetDateTime,
}