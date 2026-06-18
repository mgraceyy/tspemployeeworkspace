use serde::{Deserialize, Serialize};
use sqlx::Type;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "pin_reset_request_status", rename_all = "snake_case")]
pub enum PinResetRequestStatus {
    Pending,
    Approved,
    Denied,
    Cancelled,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PinResetRequest {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub reason: Option<String>,
    pub status: PinResetRequestStatus,
    pub requested_at: OffsetDateTime,
    pub reviewed_by: Option<Uuid>,
    pub reviewed_at: Option<OffsetDateTime>,
    pub review_note: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PinResetRequestRow {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub reason: Option<String>,
    pub requested_at: OffsetDateTime,
}
