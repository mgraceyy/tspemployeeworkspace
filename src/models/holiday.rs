use serde::Serialize;
use time::Date;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CompanyHoliday {
    pub id: Uuid,
    pub holiday_date: Date,
    pub name: String,
}
