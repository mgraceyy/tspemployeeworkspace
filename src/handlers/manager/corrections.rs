use axum::{
    extract::{Path, Query, State},
    Form,
};
use minijinja::context;
use serde::Deserialize;
use time::Date;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::display::{correction_form, CorrectionFormData, CorrectionFormInput};
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, HtmlPage, PageOrRedirect};
use crate::services::{
    corrections::{correct_entry, create_corrected_entry, CorrectionSubmission},
    settings::get_settings,
    team::{assert_can_manage, get_employee_summary, get_entry_if_manageable},
    timezone::{company_date_now, parse_date, parse_time_on_date},
};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct NewCorrectionQuery {
    date: Option<String>,
}

pub async fn new_correction_form(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
    Query(query): Query<NewCorrectionQuery>,
) -> AppResult<HtmlPage> {
    let is_admin = user.role.is_admin();
    assert_can_manage(&state.pool, user.employee_id, employee_id, is_admin).await?;

    let settings = get_settings(&state.pool).await?;
    let employee = get_employee_summary(&state.pool, employee_id).await?;
    let today = company_date_now(&settings)?;
    let work_date = parse_form_date(query.date.as_deref(), today)?;

    let form = correction_form(CorrectionFormInput {
        entry_id: None,
        employee_id,
        employee_name: &employee.full_name,
        work_date,
        clock_in: None,
        clock_out: None,
        is_new: true,
        tz: &settings.timezone,
    });
    render_correction_page(&state, &session, user, &settings.company_name, form, None).await
}

pub async fn correct_form(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(entry_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let is_admin = user.role.is_admin();

    let settings = get_settings(&state.pool).await?;
    let entry = get_entry_if_manageable(&state.pool, entry_id, user.employee_id, is_admin).await?;
    let employee = get_employee_summary(&state.pool, entry.employee_id).await?;

    let form = correction_form(CorrectionFormInput {
        entry_id: Some(entry_id),
        employee_id: entry.employee_id,
        employee_name: &employee.full_name,
        work_date: entry.work_date,
        clock_in: entry.clock_in,
        clock_out: entry.clock_out,
        is_new: false,
        tz: &settings.timezone,
    });
    render_correction_page(&state, &session, user, &settings.company_name, form, None).await
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
    AuthUser(user): AuthUser,
    Form(form): Form<CorrectionForm>,
) -> AppResult<PageOrRedirect> {
    let is_admin = user.role.is_admin();

    let settings = get_settings(&state.pool).await?;
    let today = company_date_now(&settings)?;
    let employee = get_employee_summary(&state.pool, form.employee_id).await?;
    let reason = form.reason.trim();
    if reason.is_empty() {
        let work_date = parse_form_date(Some(&form.work_date), today)?;
        let form_data = correction_form(CorrectionFormInput {
            entry_id: form.entry_id,
            employee_id: form.employee_id,
            employee_name: &employee.full_name,
            work_date,
            clock_in: None,
            clock_out: None,
            is_new: form.entry_id.is_none(),
            tz: &settings.timezone,
        });
        let page = render_correction_page(
            &state,
            &session,
            user,
            &settings.company_name,
            form_data,
            Some("Reason is required".to_string()),
        )
        .await?;
        return Ok(PageOrRedirect::Page(page));
    }

    let work_date = parse_form_date(Some(&form.work_date), today)?;
    let clock_in = parse_time_on_date(work_date, &form.clock_in, &settings.timezone)
        .map_err(AppError::bad_request)?;
    let clock_out = parse_time_on_date(work_date, &form.clock_out, &settings.timezone)
        .map_err(AppError::bad_request)?;

    let submission = CorrectionSubmission {
        editor_id: user.employee_id,
        manager_id: user.employee_id,
        is_admin,
        new_clock_in: clock_in,
        new_clock_out: clock_out,
        reason,
    };
    let result = if let Some(entry_id) = form.entry_id {
        correct_entry(&state.pool, entry_id, &submission).await
    } else {
        create_corrected_entry(&state.pool, form.employee_id, work_date, &submission).await
    };

    match result {
        Ok(_) => {
            let url = format!("/manager/team/{}", form.employee_id);
            let redirect =
                redirect_with_flash(&session, &url, "success", "Correction saved").await?;
            Ok(PageOrRedirect::Redirect(redirect))
        }
        Err(e) => {
            let form_data = correction_form(CorrectionFormInput {
                entry_id: form.entry_id,
                employee_id: form.employee_id,
                employee_name: &employee.full_name,
                work_date,
                clock_in: Some(clock_in),
                clock_out: Some(clock_out),
                is_new: form.entry_id.is_none(),
                tz: &settings.timezone,
            });
            let msg = match e {
                AppError::BadRequest(m) => Some(m),
                _ => Some("Could not save correction".into()),
            };
            let page = render_correction_page(
                &state,
                &session,
                user,
                &settings.company_name,
                form_data,
                msg,
            )
            .await?;
            Ok(PageOrRedirect::Page(page))
        }
    }
}

async fn render_correction_page(
    state: &AppState,
    session: &Session,
    user: crate::auth::UserSession,
    company_name: &str,
    form: CorrectionFormData,
    error: Option<String>,
) -> AppResult<HtmlPage> {
    render_page(
        state,
        session,
        Some(user),
        company_name,
        "Correct Time Entry",
        "manager/correct.html",
        context! {
            form => form,
            error => error,
        },
    )
    .await
}

fn parse_form_date(value: Option<&str>, default: Date) -> AppResult<Date> {
    let Some(value) = value else {
        return Ok(default);
    };
    parse_date(value).map_err(AppError::bad_request)
}
