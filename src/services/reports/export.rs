use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::TimeEntry;

use super::payroll::{minutes_to_hours_decimal, payable_minutes, PayrollDetailRow, PayrollRow};
use super::period::format_short_date;

pub fn build_payroll_detail_csv(
    period_label: &str,
    rows: &[PayrollDetailRow],
    timezone: &str,
) -> AppResult<Vec<u8>> {
    use crate::display::entry_row;

    let mut csv_bytes = Vec::new();
    {
        let mut writer = csv::Writer::from_writer(&mut csv_bytes);
        writer
            .write_record(["Pay period", period_label])
            .map_err(|e| AppError::Internal(e.into()))?;
        writer
            .write_record([
                "Employee Code",
                "Name",
                "Department",
                "Date",
                "Clock In",
                "Clock Out",
                "Regular (min)",
                "OT (min)",
                "OT Status",
                "Attendance",
            ])
            .map_err(|e| AppError::Internal(e.into()))?;

        for row in rows {
            let entry = TimeEntry {
                id: Uuid::nil(),
                employee_id: Uuid::nil(),
                work_date: row.work_date,
                clock_in: row.clock_in,
                clock_out: row.clock_out,
                gross_minutes: None,
                net_minutes: None,
                regular_minutes: row.regular_minutes,
                ot_minutes: row.ot_minutes,
                ot_status: row.ot_status,
                ot_reviewed_by: None,
                ot_reviewed_at: None,
                ot_note: None,
                ot_request_reason: None,
                attendance: row.attendance,
                created_at: OffsetDateTime::UNIX_EPOCH,
            };
            let display = entry_row(&entry, timezone);
            writer
                .write_record([
                    row.employee_code.clone(),
                    row.full_name.clone(),
                    row.department.clone().unwrap_or_default(),
                    display.work_date,
                    display.clock_in,
                    display.clock_out,
                    display.regular,
                    display.ot,
                    display.ot_status,
                    display.attendance,
                ])
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        writer.flush().map_err(|e| AppError::Internal(e.into()))?;
    }
    Ok(csv_bytes)
}

pub fn build_timesheet_csv(
    employee_code: &str,
    full_name: &str,
    start: Date,
    end: Date,
    entries: &[TimeEntry],
    timezone: &str,
) -> AppResult<Vec<u8>> {
    use crate::display::entry_row;

    let mut csv_bytes = Vec::new();
    {
        let mut writer = csv::Writer::from_writer(&mut csv_bytes);
        writer
            .write_record(["Employee Code", employee_code])
            .map_err(|e| AppError::Internal(e.into()))?;
        writer
            .write_record(["Name", full_name])
            .map_err(|e| AppError::Internal(e.into()))?;
        writer
            .write_record([
                "Period",
                &format!("{} to {}", format_short_date(start), format_short_date(end)),
            ])
            .map_err(|e| AppError::Internal(e.into()))?;
        writer
            .write_record([
                "Date",
                "Clock In",
                "Clock Out",
                "Regular (min)",
                "OT (min)",
                "OT Status",
                "Attendance",
            ])
            .map_err(|e| AppError::Internal(e.into()))?;

        for entry in entries {
            let row = entry_row(entry, timezone);
            writer
                .write_record([
                    row.work_date,
                    row.clock_in,
                    row.clock_out,
                    row.regular,
                    row.ot,
                    row.ot_status,
                    row.attendance,
                ])
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        writer.flush().map_err(|e| AppError::Internal(e.into()))?;
    }
    Ok(csv_bytes)
}

pub fn build_payroll_xlsx(
    company_name: &str,
    period_label: &str,
    rows: &[PayrollRow],
) -> AppResult<Vec<u8>> {
    use rust_xlsxwriter::{Format, Workbook};

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet
        .set_name("Payroll")
        .map_err(|e| AppError::Internal(e.into()))?;

    let title_format = Format::new().set_bold();
    worksheet
        .write_string_with_format(0, 0, company_name, &title_format)
        .map_err(|e| AppError::Internal(e.into()))?;
    worksheet
        .write_string(1, 0, format!("Pay period: {period_label}"))
        .map_err(|e| AppError::Internal(e.into()))?;

    let headers = [
        "Employee Code",
        "Name",
        "Department",
        "Regular Hours",
        "Approved OT Hours",
        "Pending OT Hours",
        "Payable Hours",
        "Sick Leave Days",
        "Vacation Days",
        "Official Leave Days",
        "Offset Days",
        "No-Show Days",
    ];
    for (col, header) in headers.iter().enumerate() {
        worksheet
            .write_string_with_format(3, col as u16, *header, &title_format)
            .map_err(|e| AppError::Internal(e.into()))?;
    }

    for (i, row) in rows.iter().enumerate() {
        let r = (4 + i) as u32;
        let payable = minutes_to_hours_decimal(payable_minutes(row));
        worksheet
            .write_string(r, 0, &row.employee_code)
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_string(r, 1, &row.full_name)
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_string(r, 2, row.department.as_deref().unwrap_or(""))
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_number(r, 3, minutes_to_hours_decimal(row.regular_minutes))
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_number(r, 4, minutes_to_hours_decimal(row.approved_ot_minutes))
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_number(r, 5, minutes_to_hours_decimal(row.pending_ot_minutes))
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_number(r, 6, payable)
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_number(r, 7, row.sick_leave_days as f64)
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_number(r, 8, row.vacation_days as f64)
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_number(r, 9, row.official_leave_days as f64)
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_number(r, 10, row.offset_days as f64)
            .map_err(|e| AppError::Internal(e.into()))?;
        worksheet
            .write_number(r, 11, row.no_show_days as f64)
            .map_err(|e| AppError::Internal(e.into()))?;
    }

    workbook
        .save_to_buffer()
        .map_err(|e| AppError::Internal(e.into()))
}
