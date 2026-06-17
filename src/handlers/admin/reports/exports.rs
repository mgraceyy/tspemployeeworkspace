use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use tower_sessions::Session;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::services::{
    reports::{
        build_payroll_detail_csv, build_payroll_xlsx, minutes_to_hours_decimal, payroll_detail,
        payroll_summary, resolve_report_period,
    },
    settings::get_settings,
    timezone::company_date_now,
};
use crate::state::AppState;

use super::page::ReportQuery;
use super::payroll_filters_from_query;

pub async fn export_detail_csv(
    State(state): State<AppState>,
    _session: Session,
    AuthUser(_user): AuthUser,
    Query(query): Query<ReportQuery>,
) -> AppResult<Response> {
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let period = resolve_report_period(
        &settings,
        today,
        query.start.as_deref(),
        query.end.as_deref(),
    )?;
    let filters = payroll_filters_from_query(&query);
    let rows = payroll_detail(&state.pool, period.start, period.end, &filters).await?;
    let csv_bytes = build_payroll_detail_csv(&period.label, &rows, &settings.timezone)?;
    let filename = format!(
        "{}-payroll-detail-{}.csv",
        settings.company_name.replace(' ', "-"),
        period.label
    );
    let disposition = format!("attachment; filename=\"{filename}\"");
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "text/csv".to_string()),
            (axum::http::header::CONTENT_DISPOSITION, disposition),
        ],
        csv_bytes,
    )
        .into_response())
}

pub async fn export_csv(
    State(state): State<AppState>,
    _session: Session,
    AuthUser(_user): AuthUser,
    Query(query): Query<ReportQuery>,
) -> AppResult<Response> {
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let period = resolve_report_period(
        &settings,
        today,
        query.start.as_deref(),
        query.end.as_deref(),
    )?;
    let filters = payroll_filters_from_query(&query);
    let rows = payroll_summary(&state.pool, period.start, period.end, &filters).await?;
    let label = period.label;

    let mut csv_bytes = Vec::new();
    {
        let mut writer = csv::Writer::from_writer(&mut csv_bytes);
        writer
            .write_record([
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
            ])
            .map_err(|e| AppError::Internal(e.into()))?;

        for row in &rows {
            writer
                .write_record([
                    row.employee_code.clone(),
                    row.full_name.clone(),
                    row.department.clone().unwrap_or_default(),
                    format!("{:.2}", minutes_to_hours_decimal(row.regular_minutes)),
                    format!("{:.2}", minutes_to_hours_decimal(row.approved_ot_minutes)),
                    format!("{:.2}", minutes_to_hours_decimal(row.pending_ot_minutes)),
                    format!(
                        "{:.2}",
                        minutes_to_hours_decimal(row.regular_minutes + row.approved_ot_minutes)
                    ),
                    row.sick_leave_days.to_string(),
                    row.vacation_days.to_string(),
                    row.official_leave_days.to_string(),
                    row.offset_days.to_string(),
                    row.no_show_days.to_string(),
                ])
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        writer.flush().map_err(|e| AppError::Internal(e.into()))?;
    }

    let filename = format!(
        "{}-payroll-{}.csv",
        settings.company_name.replace(' ', "-"),
        label
    );

    let disposition = format!("attachment; filename=\"{filename}\"");
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "text/csv".to_string()),
            (axum::http::header::CONTENT_DISPOSITION, disposition),
        ],
        csv_bytes,
    )
        .into_response())
}

pub async fn export_xlsx(
    State(state): State<AppState>,
    _session: Session,
    AuthUser(_user): AuthUser,
    Query(query): Query<ReportQuery>,
) -> AppResult<Response> {
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let period = resolve_report_period(
        &settings,
        today,
        query.start.as_deref(),
        query.end.as_deref(),
    )?;
    let filters = payroll_filters_from_query(&query);
    let rows = payroll_summary(&state.pool, period.start, period.end, &filters).await?;
    let label = period.label;

    let xlsx_bytes = build_payroll_xlsx(&settings.company_name, &label, &rows)?;
    let filename = format!(
        "{}-payroll-{}.xlsx",
        settings.company_name.replace(' ', "-"),
        label
    );
    let disposition = format!("attachment; filename=\"{filename}\"");

    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
            ),
            (axum::http::header::CONTENT_DISPOSITION, disposition),
        ],
        xlsx_bytes,
    )
        .into_response())
}
