use time::{Date, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "payroll_run_status", rename_all = "snake_case")]
pub enum PayrollRunStatus {
    Draft,
    Finalized,
    Voided,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PayrollRun {
    pub id: Uuid,
    pub period_start: Date,
    pub period_end: Date,
    pub status: PayrollRunStatus,
    pub note: Option<String>,
    pub created_by: Uuid,
    pub created_at: OffsetDateTime,
    pub finalized_at: Option<OffsetDateTime>,
    pub finalized_by: Option<Uuid>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PayrollLine {
    pub id: Uuid,
    pub run_id: Uuid,
    pub employee_id: Uuid,
    pub regular_minutes: i32,
    pub approved_ot_minutes: i32,
    pub pending_ot_minutes: i32,
    pub no_show_days: i32,
    pub base_pay_cents: i64,
    pub no_show_deduction_cents: i64,
    pub ot_pay_cents: i64,
    pub gross_pay_cents: i64,
    pub net_pay_cents: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PayrollLineWithEmployee {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub department: Option<String>,
    pub regular_minutes: i32,
    pub approved_ot_minutes: i32,
    pub pending_ot_minutes: i32,
    pub no_show_days: i32,
    pub base_pay_cents: i64,
    pub no_show_deduction_cents: i64,
    pub ot_pay_cents: i64,
    pub gross_pay_cents: i64,
    pub net_pay_cents: i64,
}
