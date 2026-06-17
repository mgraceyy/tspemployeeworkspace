use time::{Date, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EmployeeProfile {
    pub employee_id: Uuid,
    pub contact_number: Option<String>,
    pub personal_email: Option<String>,
    pub birthdate: Option<Date>,
    pub address: Option<String>,
    pub emergency_contact_name: Option<String>,
    pub emergency_contact_phone: Option<String>,
    pub job_title: Option<String>,
    pub department: Option<String>,
    pub employment_type: Option<String>,
    pub date_hired: Option<Date>,
    pub work_location: Option<String>,
    pub updated_at: OffsetDateTime,
    pub updated_by: Option<Uuid>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EmployeeWorkProfile {
    pub employee_id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub job_title: Option<String>,
    pub department: Option<String>,
    pub employment_type: Option<String>,
    pub date_hired: Option<Date>,
    pub work_location: Option<String>,
}
