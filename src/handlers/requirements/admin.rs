use axum::{
    extract::{Path, State},
    response::{Redirect, Response},
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
use crate::models::RequirementStatus;
use crate::services::{
    audit::log_action,
    employees::find_by_id,
    requirements::{
        create_type, has_uploaded_file, is_requirement_expired, list_for_employee, list_types,
        read_requirement_file, review_requirement, seed_new_type_for_all_employees, update_type,
        RequirementTypeUpdate,
    },
    settings::get_settings,
    timezone::format_time,
};
use crate::state::AppState;

use super::common::{format_file_size, requirement_file_response, status_display};

pub async fn admin_types_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let types = list_types(&state.pool).await?;

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Requirement Types",
        "admin/requirements.html",
        context! { types => types },
    )
    .await
}

#[derive(Deserialize)]
pub struct RequirementTypeForm {
    name: String,
    description: Option<String>,
    is_required: Option<String>,
    requires_upload: Option<String>,
    sort_order: Option<i32>,
    is_active: Option<String>,
    expires_after_days: Option<i32>,
    type_id: Option<Uuid>,
}

pub async fn save_requirement_type(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<RequirementTypeForm>,
) -> AppResult<Redirect> {
    let is_required = form.is_required.is_some();
    let requires_upload = form.requires_upload.is_some();
    let sort_order = form.sort_order.unwrap_or(0);

    let expires_after_days = form.expires_after_days.filter(|d| *d > 0);

    if let Some(type_id) = form.type_id {
        update_type(
            &state.pool,
            &RequirementTypeUpdate {
                type_id,
                name: &form.name,
                description: form.description.as_deref().unwrap_or(""),
                is_required,
                requires_upload,
                is_active: form.is_active.is_some(),
                sort_order,
                expires_after_days,
            },
        )
        .await?;
    } else {
        let created = create_type(
            &state.pool,
            &form.name,
            form.description.as_deref().unwrap_or(""),
            is_required,
            requires_upload,
            sort_order,
            expires_after_days,
        )
        .await?;
        seed_new_type_for_all_employees(&state.pool, created.id).await?;
    }

    log_action(
        &state.pool,
        user.employee_id,
        "requirements.type_saved",
        &format!("Saved requirement type {}", form.name.trim()),
    )
    .await?;

    redirect_with_flash(
        &session,
        "/admin/requirements",
        "success",
        "Requirement type saved",
    )
    .await
}

pub async fn admin_employee_requirements(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let employee = find_by_id(&state.pool, employee_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let reqs = list_for_employee(&state.pool, employee_id).await?;

    let rows: Vec<_> = reqs
        .iter()
        .map(|r| {
            let (status, _) = status_display(r);
            context! {
                id => r.id,
                name => r.type_name.clone(),
                description => r.type_description.clone(),
                status => status,
                employee_note => r.employee_note.clone().unwrap_or_default(),
                admin_note => r.admin_note.clone().unwrap_or_default(),
                submitted_at => r.submitted_at.map(|dt| format_time(dt, &settings.timezone)).unwrap_or_default(),
                expires_at => r.expires_at.map(|dt| format_time(dt, &settings.timezone)).unwrap_or_default(),
                is_expired => is_requirement_expired(r.expires_at),
                can_review => r.status == RequirementStatus::Submitted,
                has_file => has_uploaded_file(r),
                file_name => r.file_name.clone().unwrap_or_default(),
                file_size => format_file_size(r.file_size),
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Employee Requirements",
        "admin/employee_requirements.html",
        context! {
            employee_id => employee_id,
            employee_code => employee.employee_code,
            full_name => employee.full_name,
            requirements => rows,
        },
    )
    .await
}

pub async fn download_admin_requirement_file(
    State(state): State<AppState>,
    AuthUser(_user): AuthUser,
    Path((employee_id, requirement_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response> {
    let (req, bytes) =
        read_requirement_file(&state.pool, &state.upload_dir, employee_id, requirement_id).await?;

    Ok(requirement_file_response(
        req.file_name,
        req.file_mime,
        bytes,
    ))
}

#[derive(Deserialize)]
pub struct ReviewRequirementForm {
    pub(crate) action: String,
    pub(crate) note: Option<String>,
}

pub async fn review_employee_requirement(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path((employee_id, requirement_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<ReviewRequirementForm>,
) -> AppResult<Redirect> {
    let employee = find_by_id(&state.pool, employee_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let approve = form.action == "approve";

    review_requirement(
        &state.pool,
        employee_id,
        requirement_id,
        user.employee_id,
        approve,
        form.note.as_deref(),
    )
    .await?;

    let action = if approve {
        "requirements.approved"
    } else {
        "requirements.rejected"
    };
    log_action(
        &state.pool,
        user.employee_id,
        action,
        &format!(
            "{} requirement for {} ({})",
            if approve { "Approved" } else { "Rejected" },
            employee.full_name,
            employee.employee_code
        ),
    )
    .await?;

    redirect_with_flash(
        &session,
        &format!("/admin/employees/{employee_id}/requirements"),
        "success",
        if approve {
            "Requirement approved"
        } else {
            "Requirement rejected"
        },
    )
    .await
}
