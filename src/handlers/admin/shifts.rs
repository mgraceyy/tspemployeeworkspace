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
use crate::handlers::flash::redirect_with_flash_from_result_urls;
use crate::services::shifts::upsert_shift;
use crate::models::ShiftTemplate;
use crate::state::AppState;

use super::common::parse_time;

pub(crate) fn shift_time_defaults(shifts: &[ShiftTemplate]) -> (String, String) {
    for day in [1i16, 2, 3, 4, 5, 0, 6] {
        if let Some(shift) = shifts.iter().find(|s| s.day_of_week == day) {
            return (
                format!(
                    "{:02}:{:02}",
                    shift.start_time.hour(),
                    shift.start_time.minute()
                ),
                format!(
                    "{:02}:{:02}",
                    shift.end_time.hour(),
                    shift.end_time.minute()
                ),
            );
        }
    }
    ("08:00".into(), "17:00".into())
}

pub(crate) fn build_shift_day_rows(shifts: &[ShiftTemplate]) -> Vec<minijinja::value::Value> {
    [
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
            short_name => &name[..3],
            start_time => existing.map(|s| format!("{:02}:{:02}", s.start_time.hour(), s.start_time.minute())).unwrap_or_else(|| "08:00".into()),
            end_time => existing.map(|s| format!("{:02}:{:02}", s.end_time.hour(), s.end_time.minute())).unwrap_or_else(|| "17:00".into()),
        }
    })
    .collect()
}

pub async fn shifts_page(
    Path(employee_id): Path<Uuid>,
) -> Redirect {
    Redirect::to(&format!("/admin/employees/{employee_id}"))
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
    let employee_url = format!("/admin/employees/{}", form.employee_id);
    let result: AppResult<()> = async {
        let start = parse_time(&form.start_time)?;
        let end = parse_time(&form.end_time)?;
        upsert_shift(&state.pool, form.employee_id, form.day_of_week, start, end).await?;
        Ok(())
    }
    .await;

    redirect_with_flash_from_result_urls(
        &session,
        &employee_url,
        &employee_url,
        "Shift schedule saved",
        result,
    )
    .await
}
