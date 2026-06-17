use crate::models::{AttendanceStatus, TimeEntry, TimeEntryWithEmployee};
use crate::services::hours::format_minutes;
use crate::services::team::TeamMemberStatus;
use crate::services::timezone::{format_date, format_time, format_time_input};

#[derive(Debug, Clone, serde::Serialize)]
pub struct TimeEntryRow {
    pub id: String,
    pub work_date: String,
    pub clock_in: String,
    pub clock_out: String,
    pub regular: String,
    pub ot: String,
    pub ot_status: String,
    pub attendance: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OtPendingRow {
    pub id: String,
    pub employee_code: String,
    pub full_name: String,
    pub work_date: String,
    pub regular: String,
    pub ot: String,
    pub clock_in: String,
    pub clock_out: String,
}

pub fn entry_row(entry: &TimeEntry) -> TimeEntryRow {
    TimeEntryRow {
        id: entry.id.to_string(),
        work_date: format_date(entry.work_date),
        clock_in: entry.clock_in.map(format_time).unwrap_or_else(|| "—".into()),
        clock_out: entry.clock_out.map(format_time).unwrap_or_else(|| "—".into()),
        regular: entry
            .regular_minutes
            .map(format_minutes)
            .unwrap_or_else(|| "—".into()),
        ot: if entry.ot_minutes > 0 {
            format_minutes(entry.ot_minutes)
        } else {
            "—".into()
        },
        ot_status: entry.ot_status.label().to_lowercase(),
        attendance: entry
            .attendance
            .map(attendance_label)
            .unwrap_or_else(|| "—".into()),
    }
}

fn attendance_label(status: AttendanceStatus) -> String {
    match status {
        AttendanceStatus::OnTime => "on time".into(),
        AttendanceStatus::Late => "late".into(),
        AttendanceStatus::Absent => "absent".into(),
        AttendanceStatus::NoShow => "no show".into(),
        AttendanceStatus::Partial => "partial".into(),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TeamStatusRow {
    pub employee_id: String,
    pub employee_code: String,
    pub full_name: String,
    pub entry_id: Option<String>,
    pub shift: String,
    pub clock_in: String,
    pub clock_out: String,
    pub status: String,
    pub status_label: String,
    pub can_mark_no_show: bool,
    pub can_correct: bool,
}

pub fn team_status_row(member: &TeamMemberStatus) -> TeamStatusRow {
    let shift = match (member.shift_start, member.shift_end) {
        (Some(s), Some(e)) => format!(
            "{:02}:{:02} – {:02}:{:02}",
            s.hour(),
            s.minute(),
            e.hour(),
            e.minute()
        ),
        _ => "—".into(),
    };

    TeamStatusRow {
        employee_id: member.employee_id.to_string(),
        employee_code: member.employee_code.clone(),
        full_name: member.full_name.clone(),
        entry_id: member.entry_id.map(|id| id.to_string()),
        shift,
        clock_in: member.clock_in.map(format_time).unwrap_or_else(|| "—".into()),
        clock_out: member.clock_out.map(format_time).unwrap_or_else(|| "—".into()),
        status: member.status.clone(),
        status_label: status_label(&member.status),
        can_mark_no_show: member.can_mark_no_show,
        can_correct: member.entry_id.is_some() || member.clock_in.is_none(),
    }
}

fn status_label(status: &str) -> String {
    match status {
        "not_started" => "Not started",
        "clocked_in" => "Clocked in",
        "completed" => "Completed",
        "absent" => "Absent",
        "no_show" => "No-show",
        _ => status,
    }
    .into()
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CorrectionFormData {
    pub entry_id: Option<String>,
    pub employee_id: String,
    pub employee_name: String,
    pub work_date: String,
    pub clock_in: String,
    pub clock_out: String,
    pub is_new: bool,
}

pub fn correction_form(
    entry_id: Option<uuid::Uuid>,
    employee_id: uuid::Uuid,
    employee_name: &str,
    work_date: time::Date,
    clock_in: Option<time::OffsetDateTime>,
    clock_out: Option<time::OffsetDateTime>,
    is_new: bool,
) -> CorrectionFormData {
    CorrectionFormData {
        entry_id: entry_id.map(|id| id.to_string()),
        employee_id: employee_id.to_string(),
        employee_name: employee_name.into(),
        work_date: format_date(work_date),
        clock_in: clock_in
            .map(format_time_input)
            .unwrap_or_else(|| "08:00".into()),
        clock_out: clock_out
            .map(format_time_input)
            .unwrap_or_else(|| "17:00".into()),
        is_new,
    }
}

pub fn ot_pending_row(entry: &TimeEntryWithEmployee) -> OtPendingRow {
    OtPendingRow {
        id: entry.id.to_string(),
        employee_code: entry.employee_code.clone(),
        full_name: entry.full_name.clone(),
        work_date: format_date(entry.work_date),
        regular: entry
            .regular_minutes
            .map(format_minutes)
            .unwrap_or_else(|| "—".into()),
        ot: format_minutes(entry.ot_minutes),
        clock_in: entry.clock_in.map(format_time).unwrap_or_else(|| "—".into()),
        clock_out: entry.clock_out.map(format_time).unwrap_or_else(|| "—".into()),
    }
}