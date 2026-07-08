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
use crate::handlers::flash::redirect_with_flash_from_result_urls;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    audit::log_action,
    employees::find_by_id,
    requirements::{
        create_type, list_types, read_requirement_file, review_requirement,
        seed_new_type_for_all_employees, update_type, RequirementTypeUpdate,
    },
    settings::get_settings,
};
use crate::state::AppState;

use super::common::requirement_file_response;

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
    let result: AppResult<()> = async {
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
        Ok(())
    }
    .await;

    redirect_with_flash_from_result_urls(
        &session,
        "/admin/requirements",
        "/admin/requirements",
        "Requirement type saved",
        result,
    )
    .await
}

pub async fn admin_employee_requirements(Path(employee_id): Path<Uuid>) -> Redirect {
    Redirect::to(&format!("/admin/employees/{employee_id}"))
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
    let approve = form.action == "approve";
    let success_message = if approve {
        "Requirement approved"
    } else {
        "Requirement rejected"
    };
    let employee_url = format!("/admin/employees/{employee_id}");

    let result: AppResult<()> = async {
        let employee = find_by_id(&state.pool, employee_id)
            .await?
            .ok_or(AppError::NotFound)?;

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
        Ok(())
    }
    .await;

    redirect_with_flash_from_result_urls(
        &session,
        &employee_url,
        &employee_url,
        success_message,
        result,
    )
    .await
}
