use axum::{
    extract::{Path, State},
    response::Redirect,
    Form,
};
use minijinja::context;
use serde::Deserialize;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::AppResult;
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    employees::list_all,
    settings::get_settings,
    shifts::{list_for_employee, upsert_shift},
};
use crate::state::AppState;

use super::common::parse_time;

pub async fn shifts_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let employees = list_all(&state.pool).await?;
    let shifts = list_for_employee(&state.pool, employee_id).await?;
    let selected = employees.iter().find(|e| e.id == employee_id);
    let day_rows: Vec<_> = [
        (0, "Sunday"),
        (1, "Monday"),
        (2, "Tuesday"),
        (3, "Wednesday"),
        (4, "Thursday"),
        (5, "Friday"),
        (6, "Saturday"),
    ]
    .into_iter()
    .map(|(day, name)| {
        let existing = shifts.iter().find(|s| s.day_of_week == day);
        context! {
            day => day,
            name => name,
            start_time => existing.map(|s| format!("{:02}:{:02}", s.start_time.hour(), s.start_time.minute())).unwrap_or_else(|| "08:00".into()),
            end_time => existing.map(|s| format!("{:02}:{:02}", s.end_time.hour(), s.end_time.minute())).unwrap_or_else(|| "17:00".into()),
        }
    })
    .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Shift Schedules",
        "admin/shifts.html",
        context! {
            employees => employees,
            selected => selected,
            day_rows => day_rows,
            message => None::<String>,
        },
    )
    .await
}

#[derive(Deserialize)]
pub struct ShiftForm {
    employee_id: Uuid,
    day_of_week: i16,
    start_time: String,
    end_time: String,
}

pub async fn save_shift(
    State(state): State<AppState>,
    session: Session,
    AuthUser(_user): AuthUser,
    Form(form): Form<ShiftForm>,
) -> AppResult<Redirect> {
    let start = parse_time(&form.start_time)?;
    let end = parse_time(&form.end_time)?;

    upsert_shift(&state.pool, form.employee_id, form.day_of_week, start, end).await?;

    redirect_with_flash(
        &session,
        &format!("/admin/shifts/{}", form.employee_id),
        "success",
        "Shift schedule saved",
    )
    .await
}
