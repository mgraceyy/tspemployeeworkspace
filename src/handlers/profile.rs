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
    profile::{
        get_profile, get_work_profile, update_admin, update_self_service, AdminProfileInput,
    },
    settings::get_settings,
    team::assert_can_manage,
    timezone::{format_date, parse_date},
};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct SelfProfileForm {
    contact_number: Option<String>,
    personal_email: Option<String>,
}

#[derive(Deserialize)]
pub struct AdminProfileForm {
    contact_number: Option<String>,
    personal_email: Option<String>,
    birthdate: Option<String>,
    address: Option<String>,
    emergency_contact_name: Option<String>,
    emergency_contact_phone: Option<String>,
    job_title: Option<String>,
    department: Option<String>,
    employment_type: Option<String>,
    date_hired: Option<String>,
    work_location: Option<String>,
}

fn profile_context(
    profile: &crate::models::EmployeeProfile,
    employee_code: &str,
    full_name: &str,
) -> minijinja::value::Value {
    context! {
        employee_code => employee_code,
        full_name => full_name,
        contact_number => profile.contact_number.clone().unwrap_or_default(),
        personal_email => profile.personal_email.clone().unwrap_or_default(),
        birthdate => profile.birthdate.map(format_date).unwrap_or_default(),
        address => profile.address.clone().unwrap_or_default(),
        emergency_contact_name => profile.emergency_contact_name.clone().unwrap_or_default(),
        emergency_contact_phone => profile.emergency_contact_phone.clone().unwrap_or_default(),
        job_title => profile.job_title.clone().unwrap_or_default(),
        department => profile.department.clone().unwrap_or_default(),
        employment_type => profile.employment_type.clone().unwrap_or_default(),
        date_hired => profile.date_hired.map(format_date).unwrap_or_default(),
        work_location => profile.work_location.clone().unwrap_or_default(),
    }
}

pub async fn my_profile(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let profile = get_profile(&state.pool, user.employee_id).await?;

    let body = context! {
        profile => profile_context(&profile, &user.employee_code, &user.full_name),
    };

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "My Profile",
        "employee/profile.html",
        body,
    )
    .await
}

pub async fn update_my_profile(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<SelfProfileForm>,
) -> AppResult<Redirect> {
    update_self_service(
        &state.pool,
        user.employee_id,
        form.contact_number.as_deref(),
        form.personal_email.as_deref(),
    )
    .await?;

    log_action(
        &state.pool,
        user.employee_id,
        "profile.self_updated",
        "Updated contact number and/or personal email",
    )
    .await?;

    redirect_with_flash(&session, "/me/profile", "success", "Profile updated").await
}

pub async fn admin_profile_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let employee = crate::services::employees::find_by_id(&state.pool, employee_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let profile = get_profile(&state.pool, employee_id).await?;

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Employee Profile",
        "admin/employee_profile.html",
        context! {
            employee_id => employee_id,
            employee_code => employee.employee_code,
            full_name => employee.full_name,
            profile => profile_context(&profile, &employee.employee_code, &employee.full_name),
        },
    )
    .await
}

pub async fn admin_update_profile(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
    Form(form): Form<AdminProfileForm>,
) -> AppResult<Redirect> {
    let birthdate = form
        .birthdate
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(parse_date)
        .transpose()
        .map_err(AppError::bad_request)?;
    let date_hired = form
        .date_hired
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(parse_date)
        .transpose()
        .map_err(AppError::bad_request)?;

    let employee = crate::services::employees::find_by_id(&state.pool, employee_id)
        .await?
        .ok_or(AppError::NotFound)?;

    update_admin(
        &state.pool,
        employee_id,
        user.employee_id,
        AdminProfileInput {
            contact_number: form.contact_number.as_deref(),
            personal_email: form.personal_email.as_deref(),
            birthdate,
            address: form.address.as_deref(),
            emergency_contact_name: form.emergency_contact_name.as_deref(),
            emergency_contact_phone: form.emergency_contact_phone.as_deref(),
            job_title: form.job_title.as_deref(),
            department: form.department.as_deref(),
            employment_type: form.employment_type.as_deref(),
            date_hired,
            work_location: form.work_location.as_deref(),
        },
    )
    .await?;

    log_action(
        &state.pool,
        user.employee_id,
        "profile.updated",
        &format!(
            "Updated profile for {} ({})",
            employee.full_name, employee.employee_code
        ),
    )
    .await?;

    redirect_with_flash(
        &session,
        &format!("/admin/employees/{employee_id}/profile"),
        "success",
        "Profile saved",
    )
    .await
}

pub async fn manager_work_profile(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let is_admin = user.role.is_admin();
    assert_can_manage(&state.pool, user.employee_id, employee_id, is_admin).await?;

    let settings = get_settings(&state.pool).await?;
    let work = get_work_profile(&state.pool, employee_id).await?;

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Employee Work Profile",
        "manager/work_profile.html",
        context! {
            employee_id => work.employee_id,
            employee_code => work.employee_code,
            full_name => work.full_name,
            job_title => work.job_title.unwrap_or_default(),
            department => work.department.unwrap_or_default(),
            employment_type => work.employment_type.unwrap_or_default(),
            date_hired => work.date_hired.map(format_date).unwrap_or_default(),
            work_location => work.work_location.unwrap_or_default(),
        },
    )
    .await
}
