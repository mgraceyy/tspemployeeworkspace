use serde::{Deserialize, Serialize};
use sqlx::Type;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "leave_request_status", rename_all = "snake_case")]
pub enum LeaveRequestStatus {
    Pending,
    Approved,
    Rejected,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "leave_request_type", rename_all = "snake_case")]
pub enum LeaveRequestType {
    SickLeave,
    Vacation,
    OfficialLeave,
    Offset,
}

impl LeaveRequestType {
    pub fn label(self) -> &'static str {
        match self {
            LeaveRequestType::SickLeave => "Sick leave",
            LeaveRequestType::Vacation => "Vacation",
            LeaveRequestType::OfficialLeave => "Official leave",
            LeaveRequestType::Offset => "Offset",
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LeaveRequest {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub start_date: Date,
    pub end_date: Date,
    pub leave_type: LeaveRequestType,
    pub reason: Option<String>,
    pub status: LeaveRequestStatus,
    pub reviewer_note: Option<String>,
    pub reviewed_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LeaveRequestWithEmployee {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub start_date: Date,
    pub end_date: Date,
    pub leave_type: LeaveRequestType,
    pub reason: Option<String>,
    pub status: LeaveRequestStatus,
    pub created_at: OffsetDateTime,
}
