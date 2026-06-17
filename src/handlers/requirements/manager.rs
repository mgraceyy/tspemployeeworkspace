use axum::{
    extract::{Path, State},
    response::{Redirect, Response},
    Form,
};
use minijinja::context;
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
        has_uploaded_file, is_requirement_expired, list_for_employee, list_pending_for_manager,
        read_requirement_file, review_requirement,
    },
    settings::get_settings,
    team::assert_can_manage,
    timezone::format_time,
};
use crate::state::AppState;

use super::admin::ReviewRequirementForm;
use super::common::{format_file_size, requirement_file_response, status_display};

pub async fn manager_requirements_queue(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let pending =
        list_pending_for_manager(&state.pool, user.employee_id, user.role.is_admin()).await?;
    let rows: Vec<_> = pending
        .iter()
        .map(|row| {
            context! {
                requirement_id => row.requirement_id,
                employee_id => row.employee_id,
                employee_code => row.employee_code.clone(),
                full_name => row.full_name.clone(),
                type_name => row.type_name.clone(),
                submitted_at => row.submitted_at.map(|dt| format_time(dt, &settings.timezone)).unwrap_or_default(),
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Team Requirements",
        "manager/requirements.html",
        context! { pending => rows },
    )
    .await
}

pub async fn manager_employee_requirements(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(employee_id): Path<Uuid>,
) -> AppResult<HtmlPage> {
    assert_can_manage(
        &state.pool,
        user.employee_id,
        employee_id,
        user.role.is_admin(),
    )
    .await?;

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
        "manager/employee_requirements.html",
        context! {
            employee_id => employee_id,
            employee_code => employee.employee_code,
            full_name => employee.full_name,
            requirements => rows,
        },
    )
    .await
}

pub async fn download_manager_requirement_file(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((employee_id, requirement_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response> {
    assert_can_manage(
        &state.pool,
        user.employee_id,
        employee_id,
        user.role.is_admin(),
    )
    .await?;

    let (req, bytes) =
        read_requirement_file(&state.pool, &state.upload_dir, employee_id, requirement_id).await?;

    Ok(requirement_file_response(
        req.file_name,
        req.file_mime,
        bytes,
    ))
}

pub async fn manager_review_employee_requirement(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path((employee_id, requirement_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<ReviewRequirementForm>,
) -> AppResult<Redirect> {
    assert_can_manage(
        &state.pool,
        user.employee_id,
        employee_id,
        user.role.is_admin(),
    )
    .await?;

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

    log_action(
        &state.pool,
        user.employee_id,
        if approve {
            "requirements.approved"
        } else {
            "requirements.rejected"
        },
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
        &format!("/manager/team/{employee_id}/requirements"),
        "success",
        if approve {
            "Requirement approved"
        } else {
            "Requirement rejected"
        },
    )
    .await
}
