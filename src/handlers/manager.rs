use axum::{
    extract::{Path, Query, State},
    response::Redirect,
    Form,
};
use minijinja::context;
use serde::Deserialize;
use time::Date;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::{get_active_session, require_manager};
use crate::display::{
    correction_form, entry_row, ot_pending_row, team_status_row, CorrectionFormData,
};
use crate::error::{AppError, AppResult};
use crate::handlers::render::{render_page, HtmlPage, PageOrRedirect};
use crate::models::EmployeeSummary;
use crate::services::{
    attendance::mark_no_show_for_employee,
    clock::list_entries_for_employee,
    corrections::{correct_entry, create_corrected_entry},
    employees::list_team,
    ot::{list_pending_for_manager, review_overtime},
    settings::get_settings,
    team::{
        assert_can_manage, get_employee_summary, get_entry_if_manageable,
        list_manageable_employees, list_team_attendance_today,
    },
    timezone::{format_date, manila_date_now, parse_time_on_date},
};
use crate::state::AppState;

pub async fn dashboard(State(state): State<AppState>, session: Session) -> AppResult<HtmlPage> {
    let user = get_active_session(&session).await?;
    require_manager(&user)?;
    let settings = get_settings(&state.pool).await?;
    let is_admin = user.role.is_admin();

    let pending_ot = list_pending_for_manager(&state.pool, user.employee_id, is_admin).await?;
    let team_today = list_team_attendance_today(&state.pool, user.employee_id, is_admin).await?;
    let team: Vec<EmployeeSummary> = if is_admin {
        list_manageable_employees(&state.pool, user.employee_id, true).await?
    } else {
        list_team(&state.pool, user.employee_id).await?
    };

    let pending_rows: Vec<_> = pending_ot.iter().map(ot_pending_row).collect();
    let attendance_rows: Vec<_> = team_today.iter().map(team_status_row).collect();

    render_page(
        &state,
        Some(user),
        &settings.company_name,
        "Manager Dashboard",
        "manager/dashboard.html",
        context! {
            pending_ot => pending_rows,
            attendance => attendance_rows,
            team => team,
            today => format_date(manila_date_now()),
        }
        .into(),
    )
    .await
}

pub async fn team_list(State(state): State<AppState>, session: Session) -> AppResult<HtmlPage> {
    let user = get_active_session(&session).await?;
    require_manager(&user)?;
    let settings = get_settings(&state.pool).await?;
    let is_admin = user.role.is_admin();
    let team = list_manageable_employees(&state.pool, user.employee_id, is_admin).await?;

    render_page(
        &state,
        Some(user),
        &settings.company_name,
        "Team",
        "manager/team_list.html",
        context! {
            team => team,
        }
        .into(),
    )
    .await
}

pub async fn team_timesheet(
    State(state): State<AppState>,
    session: Session,
    Path(employee_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let user = get_active_session(&session).await?;
    require_manager(&user)?;
    let is_admin = user.role.is_admin();
    assert_can_manage(&state.pool, user.employee_id, employee_id, is_admin).await?;

    let settings = get_settings(&state.pool).await?;
    let employee = get_employee_summary(&state.pool, employee_id).await?;
    let entries = list_entries_for_employee(&state.pool, employee_id, 30).await?;
    let rows: Vec<_> = entries.iter().map(entry_row).collect();

    render_page(
        &state,
        Some(user),
        &settings.company_name,
        "Team Timesheet",
        "manager/team_timesheet.html",
        context! {
            employee => employee,
            entries => rows,
        }
        .into(),
    )
    .await
}

#[derive(Deserialize)]
pub struct NewCorrectionQuery {
    date: Option<String>,
}

pub async fn new_correction_form(
    State(state): State<AppState>,
    session: Session,
    Path(employee_id): Path<Uuid>,
    Query(query): Query<NewCorrectionQuery>,
) -> AppResult<HtmlPage> {
    let user = get_active_session(&session).await?;
    require_manager(&user)?;
    let is_admin = user.role.is_admin();
    assert_can_manage(&state.pool, user.employee_id, employee_id, is_admin).await?;

    let settings = get_settings(&state.pool).await?;
    let employee = get_employee_summary(&state.pool, employee_id).await?;
    let work_date = parse_form_date(query.date.as_deref())?;

    let form = correction_form(None, employee_id, &employee.full_name, work_date, None, None, true);
    render_correction_page(&state, user, &settings.company_name, form, None).await
}

pub async fn correct_form(
    State(state): State<AppState>,
    session: Session,
    Path(entry_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let user = get_active_session(&session).await?;
    require_manager(&user)?;
    let is_admin = user.role.is_admin();

    let settings = get_settings(&state.pool).await?;
    let entry = get_entry_if_manageable(&state.pool, entry_id, user.employee_id, is_admin).await?;
    let employee = get_employee_summary(&state.pool, entry.employee_id).await?;

    let form = correction_form(
        Some(entry_id),
        entry.employee_id,
        &employee.full_name,
        entry.work_date,
        entry.clock_in,
        entry.clock_out,
        false,
    );
    render_correction_page(&state, user, &settings.company_name, form, None).await
}

#[derive(Deserialize)]
pub struct CorrectionForm {
    entry_id: Option<Uuid>,
    employee_id: Uuid,
    work_date: String,
    clock_in: String,
    clock_out: String,
    reason: String,
}

pub async fn submit_correction(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<CorrectionForm>,
) -> AppResult<PageOrRedirect> {
    let user = get_active_session(&session).await?;
    require_manager(&user)?;
    let is_admin = user.role.is_admin();

    let settings = get_settings(&state.pool).await?;
    let employee = get_employee_summary(&state.pool, form.employee_id).await?;
    let reason = form.reason.trim();
    if reason.is_empty() {
        let work_date = parse_form_date(Some(&form.work_date))?;
        let form_data = correction_form(
            form.entry_id,
            form.employee_id,
            &employee.full_name,
            work_date,
            None,
            None,
            form.entry_id.is_none(),
        );
        let page = render_correction_page(
            &state,
            user,
            &settings.company_name,
            form_data,
            Some("Reason is required".to_string()),
        )
        .await?;
        return Ok(PageOrRedirect::Page(page));
    }

    let work_date = parse_form_date(Some(&form.work_date))?;
    let clock_in = parse_time_on_date(work_date, &form.clock_in)
        .map_err(|m| AppError::bad_request(m))?;
    let clock_out = parse_time_on_date(work_date, &form.clock_out)
        .map_err(|m| AppError::bad_request(m))?;

    let result = if let Some(entry_id) = form.entry_id {
        correct_entry(
            &state.pool,
            entry_id,
            user.employee_id,
            clock_in,
            clock_out,
            reason,
            is_admin,
            user.employee_id,
        )
        .await
    } else {
        create_corrected_entry(
            &state.pool,
            form.employee_id,
            work_date,
            user.employee_id,
            clock_in,
            clock_out,
            reason,
            is_admin,
            user.employee_id,
        )
        .await
    };

    match result {
        Ok(_) => Ok(PageOrRedirect::Redirect(Redirect::to(&format!(
            "/manager/team/{}",
            form.employee_id
        )))),
        Err(e) => {
            let form_data = correction_form(
                form.entry_id,
                form.employee_id,
                &employee.full_name,
                work_date,
                Some(clock_in),
                Some(clock_out),
                form.entry_id.is_none(),
            );
            let msg = match e {
                AppError::BadRequest(m) => Some(m),
                _ => Some("Could not save correction".into()),
            };
            let page =
                render_correction_page(&state, user, &settings.company_name, form_data, msg)
                    .await?;
            Ok(PageOrRedirect::Page(page))
        }
    }
}

// Fix submit_correction to return PageOrRedirect or separate types

#[derive(Deserialize)]
pub struct OtReviewForm {
    action: String,
    note: Option<String>,
}

pub async fn review_ot(
    State(state): State<AppState>,
    session: Session,
    Path(entry_id): Path<Uuid>,
    Form(form): Form<OtReviewForm>,
) -> AppResult<Redirect> {
    let user = get_active_session(&session).await?;
    require_manager(&user)?;

    let approve = form.action == "approve";
    review_overtime(
        &state.pool,
        entry_id,
        user.employee_id,
        approve,
        form.note.filter(|n| !n.trim().is_empty()),
        user.role.is_admin(),
    )
    .await?;

    Ok(Redirect::to("/manager"))
}

#[derive(Deserialize)]
pub struct NoShowForm {
    employee_id: Uuid,
}

pub async fn mark_no_show(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<NoShowForm>,
) -> AppResult<Redirect> {
    let user = get_active_session(&session).await?;
    require_manager(&user)?;
    let today = manila_date_now();

    mark_no_show_for_employee(
        &state.pool,
        form.employee_id,
        today,
        user.employee_id,
        user.role.is_admin(),
        user.employee_id,
    )
    .await?;

    Ok(Redirect::to("/manager"))
}

async fn render_correction_page(
    state: &AppState,
    user: crate::auth::UserSession,
    company_name: &str,
    form: CorrectionFormData,
    error: Option<String>,
) -> AppResult<HtmlPage> {
    render_page(
        state,
        Some(user),
        company_name,
        "Correct Time Entry",
        "manager/correct.html",
        context! {
            form => form,
            error => error,
        }
        .into(),
    )
    .await
}

fn parse_form_date(value: Option<&str>) -> AppResult<Date> {
    let Some(value) = value else {
        return Ok(manila_date_now());
    };
    let parts: Vec<_> = value.split('-').collect();
    if parts.len() != 3 {
        return Err(AppError::bad_request("Invalid date"));
    }
    let year: i32 = parts[0].parse().map_err(|_| AppError::bad_request("Invalid year"))?;
    let month: u8 = parts[1].parse().map_err(|_| AppError::bad_request("Invalid month"))?;
    let day: u8 = parts[2].parse().map_err(|_| AppError::bad_request("Invalid day"))?;
    let month = time::Month::try_from(month).map_err(|_| AppError::bad_request("Invalid month"))?;
    Date::from_calendar_date(year, month, day).map_err(|_| AppError::bad_request("Invalid date"))
}