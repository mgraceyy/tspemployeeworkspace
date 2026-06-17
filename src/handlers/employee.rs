use axum::{
    extract::State,
    response::Redirect,
};
use minijinja::context;
use tower_sessions::Session;

use crate::auth::get_active_session;
use crate::display::entry_row;
use crate::error::AppResult;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    attendance::get_shift_for_date,
    clock::{clock_in, clock_out, get_today_entry, list_entries_for_employee},
    settings::get_settings,
    timezone::{format_date, format_time, manila_date_now, now_manila},
};
use crate::state::AppState;

pub async fn home(State(state): State<AppState>, session: Session) -> AppResult<HtmlPage> {
    let user = get_active_session(&session).await?;
    let settings = get_settings(&state.pool).await?;
    let today = manila_date_now();
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
    let entry_display = entry.as_ref().map(entry_row);

    render_page(
        &state,
        Some(user),
        &settings.company_name,
        "Clock In / Out",
        "employee/clock.html",
        context! {
            today => format_date(today),
            now => format_time(now_manila()),
            entry => entry_display,
            shift => shift_display,
            status => status,
            message => None::<String>,
        }
        .into(),
    )
    .await
}

pub async fn clock_in_action(
    State(state): State<AppState>,
    session: Session,
) -> AppResult<Redirect> {
    let user = get_active_session(&session).await?;
    clock_in(&state.pool, user.employee_id).await?;
    Ok(Redirect::to("/"))
}

pub async fn clock_out_action(
    State(state): State<AppState>,
    session: Session,
) -> AppResult<Redirect> {
    let user = get_active_session(&session).await?;
    clock_out(&state.pool, user.employee_id).await?;
    Ok(Redirect::to("/"))
}

pub async fn timesheet(State(state): State<AppState>, session: Session) -> AppResult<HtmlPage> {
    let user = get_active_session(&session).await?;
    let settings = get_settings(&state.pool).await?;
    let entries = list_entries_for_employee(&state.pool, user.employee_id, 30).await?;
    let rows: Vec<_> = entries.iter().map(entry_row).collect();

    render_page(
        &state,
        Some(user),
        &settings.company_name,
        "My Timesheet",
        "employee/timesheet.html",
        context! {
            entries => rows,
        }
        .into(),
    )
    .await
}