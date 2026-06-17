use serde::{Deserialize, Serialize};
use sqlx::Type;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "user_role", rename_all = "snake_case")]
pub enum UserRole {
    Employee,
    Manager,
    Admin,
}

impl UserRole {
    pub fn is_manager_or_admin(self) -> bool {
        matches!(self, UserRole::Manager | UserRole::Admin)
    }

    pub fn is_admin(self) -> bool {
        matches!(self, UserRole::Admin)
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Employee {
    pub id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub pin_hash: String,
    pub role: UserRole,
    pub manager_id: Option<Uuid>,
    pub is_active: bool,
    pub must_change_pin: bool,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct EmployeeSummary {
    pub id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub role: UserRole,
    pub manager_id: Option<Uuid>,
    pub is_active: bool,
}