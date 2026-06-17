use serde::{Deserialize, Serialize};
use sqlx::Type;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "eod_report_status", rename_all = "snake_case")]
pub enum EodReportStatus {
    Draft,
    Submitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "eod_task_kind", rename_all = "snake_case")]
pub enum EodTaskKind {
    Completed,
    Pending,
    Blocked,
    Planned,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EodReport {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub report_date: Date,
    pub summary: String,
    pub status: EodReportStatus,
    pub submitted_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EodTask {
    pub id: Uuid,
    pub eod_report_id: Uuid,
    pub kind: EodTaskKind,
    pub title: String,
    pub description: String,
    pub sort_order: i32,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EodHistoryItem {
    pub id: Uuid,
    pub report_date: Date,
    pub summary: String,
    pub submitted_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EodReportSummary {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub department: Option<String>,
    pub report_date: Date,
    pub summary: String,
    pub status: EodReportStatus,
    pub submitted_at: Option<OffsetDateTime>,
}
