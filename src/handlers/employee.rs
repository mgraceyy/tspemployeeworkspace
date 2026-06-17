use axum::{
    extract::{Form, Query, State},
    http::header,
    response::{IntoResponse, Redirect, Response},
};
use minijinja::context;
use serde::Deserialize;
use time::Duration;
use tower_sessions::Session;

use crate::auth::AuthUser;
use crate::display::entry_row;
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    attendance::get_shift_for_date,
    clock::{clock_in, clock_out, get_today_entry, list_entries_for_employee_range},
    eod::needs_eod_reminder,
    holidays::{is_holiday, list_holidays_between},
    reports::{build_timesheet_csv, resolve_timesheet_period},
    settings::get_settings,
    timezone::{company_date_now, format_date, format_time, now_company},
};
use crate::state::AppState;

pub async fn home(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let entry = get_today_entry(&state.pool, user.employee_id).await?;
    let shift = get_shift_for_date(&state.pool, user.employee_id, today).await?;

    let status = match &entry {
        None => "not_started",
        Some(e) if e.clock_in.is_some() && e.clock_out.is_none() => "clocked_in",
        Some(e) if e.clock_out.is_some() => "completed",
        _ => "not_started",
    };

    let shift_display = shift.as_ref().map(|s| {
        context! {
            start_time => format!("{:02}:{:02}", s.start_time.hour(), s.start_time.minute()),
            end_time => format!("{:02}:{:02}", s.end_time.hour(), s.end_time.minute()),
        }
    });
    let tz = settings.timezone.as_str();
    let entry_display = entry.as_ref().map(|e| entry_row(e, tz));
    let eod_due = needs_eod_reminder(&state.pool, user.employee_id).await?;
    let holiday_today = is_holiday(&state.pool, today).await?;
    let upcoming_holidays =
        list_holidays_between(&state.pool, today, today + Duration::days(90)).await?;
    let holiday_preview: Vec<_> = upcoming_holidays
        .iter()
        .take(3)
        .map(|holiday| {
            context! {
                date => format_date(holiday.holiday_date),
                name => holiday.name.clone(),
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Clock In / Out",
        "employee/clock.html",
        context! {
            today => format_date(today),
            now => format_time(now_company(&settings)?, tz),
            entry => entry_display,
            shift => shift_display,
            status => status,
            eod_due => eod_due,
            holiday_today => holiday_today,
            upcoming_holidays => holiday_preview,
            ot_requires_approval => settings.ot_requires_approval,
        },
    )
    .await
}

pub async fn clock_in_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<Redirect> {
    clock_in(&state.pool, user.employee_id).await?;
    redirect_with_flash(&session, "/", "success", "Clocked in successfully").await
}

#[derive(Deserialize, Default)]
pub struct ClockOutForm {
    ot_reason: Option<String>,
}

pub async fn clock_out_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<ClockOutForm>,
) -> AppResult<Redirect> {
    match clock_out(&state.pool, user.employee_id, form.ot_reason.as_deref()).await {
        Ok(_) => redirect_with_flash(&session, "/", "success", "Clocked out successfully").await,
        Err(AppError::BadRequest(msg)) => redirect_with_flash(&session, "/", "error", &msg).await,
        Err(err) => Err(err),
    }
}

#[derive(Deserialize, Default)]
pub struct TimesheetQuery {
    start: Option<String>,
    end: Option<String>,
}

pub async fn timesheet(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Query(query): Query<TimesheetQuery>,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let (start, end) =
        resolve_timesheet_period(today, query.start.as_deref(), query.end.as_deref())?;
    let entries =
        list_entries_for_employee_range(&state.pool, user.employee_id, start, end).await?;
    let tz = settings.timezone.as_str();
    let rows: Vec<_> = entries.iter().map(|e| entry_row(e, tz)).collect();
    let export_query = if query.start.is_some() && query.end.is_some() {
        format!("?start={}&end={}", format_date(start), format_date(end))
    } else {
        String::new()
    };

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "My Timesheet",
        "employee/timesheet.html",
        context! {
            entries => rows,
            start_date => format_date(start),
            end_date => format_date(end),
            export_query => export_query,
        },
    )
    .await
}

pub async fn holidays_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let end = today + Duration::days(365);
    let holidays = list_holidays_between(&state.pool, today, end).await?;
    let rows: Vec<_> = holidays
        .iter()
        .map(|holiday| {
            context! {
                date => format_date(holiday.holiday_date),
                name => holiday.name.clone(),
                is_today => holiday.holiday_date == today,
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Company Holidays",
        "employee/holidays.html",
        context! { holidays => rows },
    )
    .await
}

pub async fn export_my_timesheet_csv(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Query(query): Query<TimesheetQuery>,
) -> AppResult<Response> {
    let employee = crate::services::employees::find_by_id(&state.pool, user.employee_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let (start, end) =
        resolve_timesheet_period(today, query.start.as_deref(), query.end.as_deref())?;
    let entries =
        list_entries_for_employee_range(&state.pool, user.employee_id, start, end).await?;
    let csv_bytes = build_timesheet_csv(
        &employee.employee_code,
        &employee.full_name,
        start,
        end,
        &entries,
        &settings.timezone,
    )?;

    let filename = format!(
        "{}-timesheet-{}-{}.csv",
        employee.employee_code,
        format_date(start),
        format_date(end)
    );
    let disposition = format!("attachment; filename=\"{filename}\"");

    let _ = session;
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv".to_string()),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        csv_bytes,
    )
        .into_response())
}
