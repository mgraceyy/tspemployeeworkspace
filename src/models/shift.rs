use serde::Serialize;
use time::Time;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ShiftTemplate {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub day_of_week: i16,
    pub start_time: Time,
    pub end_time: Time,
}
