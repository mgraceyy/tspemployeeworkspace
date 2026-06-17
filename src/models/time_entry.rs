use serde::{Deserialize, Serialize};
use sqlx::Type;
use time::Date;
use uuid::Uuid;

use super::employee::EmployeeSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[sqlx(type_name = "attendance_status", rename_all = "snake_case")]
pub enum AttendanceStatus {
    OnTime,
    Late,
    Absent,
    NoShow,
    Partial,
    SickLeave,
    Vacation,
    OfficialLeave,
    Offset,
}

impl AttendanceStatus {
    pub fn is_planned_leave(self) -> bool {
        matches!(
            self,
            AttendanceStatus::SickLeave
                | AttendanceStatus::Vacation
                | AttendanceStatus::OfficialLeave
                | AttendanceStatus::Offset
        )
    }

    pub fn is_manager_markable(self) -> bool {
        matches!(
            self,
            AttendanceStatus::NoShow
                | AttendanceStatus::SickLeave
                | AttendanceStatus::Vacation
                | AttendanceStatus::OfficialLeave
                | AttendanceStatus::Offset
        )
    }

    pub fn is_unexcused_absence(self) -> bool {
        matches!(self, AttendanceStatus::NoShow | AttendanceStatus::Absent)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[sqlx(type_name = "ot_status", rename_all = "snake_case")]
pub enum OtStatus {
    None,
    Pending,
    Approved,
    Rejected,
}

impl OtStatus {
    pub fn label(self) -> &'static str {
        match self {
            OtStatus::None => "—",
            OtStatus::Pending => "Pending",
            OtStatus::Approved => "Approved",
            OtStatus::Rejected => "Rejected",
        }
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TimeEntry {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub work_date: Date,
    pub clock_in: Option<time::OffsetDateTime>,
    pub clock_out: Option<time::OffsetDateTime>,
    pub gross_minutes: Option<i32>,
    pub net_minutes: Option<i32>,
    pub regular_minutes: Option<i32>,
    pub ot_minutes: i32,
    pub ot_status: OtStatus,
    pub ot_reviewed_by: Option<Uuid>,
    pub ot_reviewed_at: Option<time::OffsetDateTime>,
    pub ot_note: Option<String>,
    pub ot_request_reason: Option<String>,
    pub attendance: Option<AttendanceStatus>,
    pub created_at: time::OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TimeEntryWithEmployee {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub work_date: Date,
    pub clock_in: Option<time::OffsetDateTime>,
    pub clock_out: Option<time::OffsetDateTime>,
    pub gross_minutes: Option<i32>,
    pub net_minutes: Option<i32>,
    pub regular_minutes: Option<i32>,
    pub ot_minutes: i32,
    pub ot_status: OtStatus,
    pub ot_note: Option<String>,
    pub ot_request_reason: Option<String>,
    pub attendance: Option<AttendanceStatus>,
}

#[derive(Debug, Clone)]
pub struct TimeEntryView {
    pub entry: TimeEntry,
    pub employee: Option<EmployeeSummary>,
}
