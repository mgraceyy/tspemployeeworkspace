use serde::{Deserialize, Serialize};
use sqlx::Type;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "requirement_status", rename_all = "snake_case")]
pub enum RequirementStatus {
    Missing,
    Submitted,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RequirementType {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub is_required: bool,
    pub requires_upload: bool,
    pub is_active: bool,
    pub sort_order: i32,
    pub expires_after_days: Option<i32>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EmployeeRequirement {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub requirement_type_id: Uuid,
    pub type_name: String,
    pub type_description: String,
    pub is_required: bool,
    pub requires_upload: bool,
    pub status: RequirementStatus,
    pub employee_note: Option<String>,
    pub admin_note: Option<String>,
    pub submitted_at: Option<OffsetDateTime>,
    pub expires_at: Option<OffsetDateTime>,
    pub file_name: Option<String>,
    pub file_stored_path: Option<String>,
    pub file_mime: Option<String>,
    pub file_size: Option<i64>,
}
