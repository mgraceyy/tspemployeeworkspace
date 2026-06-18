use axum::{
    body::Body,
    extract::{Multipart, Path, State},
    http::header,
    response::{IntoResponse, Redirect, Response},
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
    employees::bump_session_version,
    pin_reset::{cancel_own_request, create_request, get_pending_for_employee},
    profile::{
        get_profile, get_work_profile, set_photo_path, update_admin, update_self_service,
        AdminProfileInput,
    },
    settings::get_settings,
    team::assert_can_manage,
    timezone::{format_date, parse_date},
    uploads::{read_stored_file, store_profile_photo},
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
    bank_account: Option<String>,
    tin: Option<String>,
    sss_number: Option<String>,
    philhealth_number: Option<String>,
}

#[derive(Deserialize)]
pub struct PinResetReasonForm {
    reason: Option<String>,
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
        bank_account => profile.bank_account.clone().unwrap_or_default(),
        tin => profile.tin.clone().unwrap_or_default(),
        sss_number => profile.sss_number.clone().unwrap_or_default(),
        philhealth_number => profile.philhealth_number.clone().unwrap_or_default(),
        has_photo => profile.photo_path.is_some(),
    }
}

pub async fn my_profile(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let profile = get_profile(&state.pool, user.employee_id).await?;
    let pending_pin_reset = get_pending_for_employee(&state.pool, user.employee_id).await?;

    let body = context! {
        profile => profile_context(&profile, &user.employee_code, &user.full_name),
        pending_pin_reset => pending_pin_reset.is_some(),
        pending_pin_reset_id => pending_pin_reset.as_ref().map(|r| r.id),
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

pub async fn upload_my_profile_photo(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    mut multipart: Multipart,
) -> AppResult<Redirect> {
    let mut bytes = None;
    let mut file_name = None;
    let mut mime = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Internal(e.into()))?
    {
        if field.name() == Some("photo") {
            file_name = field.file_name().map(str::to_string);
            mime = field.content_type().map(str::to_string);
            bytes = Some(
                field
                    .bytes()
                    .await
                    .map_err(|e| AppError::Internal(e.into()))?,
            );
        }
    }

    let bytes = bytes.ok_or_else(|| AppError::bad_request("Photo file is required"))?;
    let stored = store_profile_photo(
        &state.upload_dir,
        user.employee_id,
        file_name.as_deref().unwrap_or("photo.jpg"),
        mime.as_deref().unwrap_or("image/jpeg"),
        &bytes,
        state.max_upload_bytes,
    )
    .await?;

    set_photo_path(
        &state.pool,
        user.employee_id,
        user.employee_id,
        Some(&stored.stored_path),
    )
    .await?;

    log_action(
        &state.pool,
        user.employee_id,
        "profile.photo_updated",
        "Updated profile photo",
    )
    .await?;

    redirect_with_flash(&session, "/me/profile", "success", "Profile photo updated").await
}

pub async fn my_profile_photo(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> AppResult<Response> {
    serve_profile_photo(&state, user.employee_id, user.employee_id, false).await
}

pub async fn employee_profile_photo(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
) -> AppResult<Response> {
    let is_admin = user.role.is_admin();
    if employee_id != user.employee_id {
        assert_can_manage(&state.pool, user.employee_id, employee_id, is_admin).await?;
    }
    serve_profile_photo(&state, employee_id, user.employee_id, true).await
}

async fn serve_profile_photo(
    state: &AppState,
    employee_id: Uuid,
    _viewer_id: Uuid,
    inline: bool,
) -> AppResult<Response> {
    let profile = get_profile(&state.pool, employee_id).await?;
    let Some(path) = profile.photo_path.as_deref() else {
        return Err(AppError::NotFound);
    };
    let bytes = read_stored_file(&state.upload_dir, path).await?;
    let mime = if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".webp") {
        "image/webp"
    } else {
        "image/jpeg"
    };
    let disposition = if inline {
        "inline"
    } else {
        "inline"
    };
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(Body::from(bytes))
        .map_err(|e| AppError::Internal(e.into()))?
        .into_response())
}

pub async fn logout_everywhere(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<Redirect> {
    bump_session_version(&state.pool, user.employee_id).await?;
    log_action(
        &state.pool,
        user.employee_id,
        "auth.logout_everywhere",
        "Signed out of all devices",
    )
    .await?;
    crate::auth::clear_session(&session).await?;
    Ok(Redirect::to("/login?signed_out=all"))
}

pub async fn request_pin_reset(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<PinResetReasonForm>,
) -> AppResult<Redirect> {
    create_request(
        &state.pool,
        user.employee_id,
        form.reason.as_deref(),
    )
    .await?;

    log_action(
        &state.pool,
        user.employee_id,
        "auth.pin_reset_requested",
        "Requested PIN reset",
    )
    .await?;

    redirect_with_flash(
        &session,
        "/me/profile",
        "success",
        "PIN reset request submitted — your manager or admin will review it",
    )
    .await
}

pub async fn cancel_pin_reset(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(request_id): Path<Uuid>,
) -> AppResult<Redirect> {
    cancel_own_request(&state.pool, user.employee_id, request_id).await?;
    redirect_with_flash(
        &session,
        "/me/profile",
        "success",
        "PIN reset request cancelled",
    )
    .await
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
            bank_account: form.bank_account.as_deref(),
            tin: form.tin.as_deref(),
            sss_number: form.sss_number.as_deref(),
            philhealth_number: form.philhealth_number.as_deref(),
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