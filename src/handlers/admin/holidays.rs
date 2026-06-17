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
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    audit::log_action,
    holidays::{add_holiday, delete_holiday, list_holidays},
    settings::get_settings,
    timezone::{format_date, parse_date},
};
use crate::state::AppState;

pub async fn holidays_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let holidays = list_holidays(&state.pool).await?;

    let rows: Vec<_> = holidays
        .iter()
        .map(|h| {
            context! {
                id => h.id,
                holiday_date => format_date(h.holiday_date),
                name => h.name.clone(),
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Company Holidays",
        "admin/holidays.html",
        context! { holidays => rows },
    )
    .await
}

#[derive(Deserialize)]
pub struct HolidayForm {
    holiday_date: String,
    name: String,
}

pub async fn add_holiday_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<HolidayForm>,
) -> AppResult<Redirect> {
    let holiday_date = parse_date(&form.holiday_date).map_err(AppError::bad_request)?;
    let created = add_holiday(&state.pool, holiday_date, &form.name).await?;

    log_action(
        &state.pool,
        user.employee_id,
        "holidays.added",
        &format!(
            "Added holiday {} on {}",
            created.name,
            format_date(created.holiday_date)
        ),
    )
    .await?;

    redirect_with_flash(&session, "/admin/holidays", "success", "Holiday added").await
}

pub async fn delete_holiday_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(holiday_id): Path<Uuid>,
) -> AppResult<Redirect> {
    delete_holiday(&state.pool, holiday_id).await?;

    log_action(
        &state.pool,
        user.employee_id,
        "holidays.deleted",
        "Removed company holiday",
    )
    .await?;

    redirect_with_flash(&session, "/admin/holidays", "success", "Holiday removed").await
}
