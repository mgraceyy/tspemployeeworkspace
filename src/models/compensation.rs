use time::Date;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CompensationProfile {
    pub employee_id: Uuid,
    pub monthly_salary_cents: i64,
    pub ot_rate_percent: i32,
    pub effective_from: Date,
}
