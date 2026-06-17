use axum::{
    extract::{Multipart, Path, State},
    response::{Redirect, Response},
};
use minijinja::context;
use tower_sessions::Session;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    requirements::{
        can_submit_requirement, has_uploaded_file, is_requirement_expired, list_for_employee,
        read_requirement_file, submit_requirement,
    },
    settings::get_settings,
    timezone::format_time,
};
use crate::state::AppState;

use super::common::{format_file_size, requirement_file_response, status_display};

pub async fn my_requirements(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let reqs = list_for_employee(&state.pool, user.employee_id).await?;
    let rows: Vec<_> = reqs
        .iter()
        .map(|r| {
            let (status, status_key) = status_display(r);
            context! {
                id => r.id,
                name => r.type_name.clone(),
                description => r.type_description.clone(),
                is_required => r.is_required,
                requires_upload => r.requires_upload,
                status => status,
                status_key => status_key,
                employee_note => r.employee_note.clone().unwrap_or_default(),
                admin_note => r.admin_note.clone().unwrap_or_default(),
                expires_at => r.expires_at.map(|dt| format_time(dt, &settings.timezone)).unwrap_or_default(),
                is_expired => is_requirement_expired(r.expires_at),
                can_submit => can_submit_requirement(r),
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
        "My Requirements",
        "employee/requirements.html",
        context! { requirements => rows },
    )
    .await
}

pub async fn submit_my_requirement(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(requirement_id): Path<Uuid>,
    mut multipart: Multipart,
) -> AppResult<Redirect> {
    let mut note = None;
    let mut upload = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(format!("Invalid upload form: {e}")))?
    {
        match field.name() {
            Some("note") => {
                note = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| AppError::bad_request(format!("Invalid note: {e}")))?,
                );
            }
            Some("file") => {
                let file_name = field
                    .file_name()
                    .map(str::to_string)
                    .filter(|name| !name.is_empty());
                let mime_type = field
                    .content_type()
                    .map(|mime| mime.to_string())
                    .unwrap_or_else(|| "application/octet-stream".to_string());
                let bytes = field.bytes().await.map_err(|e| {
                    AppError::bad_request(format!("Could not read uploaded file: {e}"))
                })?;
                if !bytes.is_empty() {
                    let original_name = file_name.unwrap_or_else(|| "upload.bin".to_string());
                    upload = Some((original_name, mime_type, bytes));
                }
            }
            _ => {}
        }
    }

    let upload_ref = upload
        .as_ref()
        .map(|(name, mime, bytes)| (name.as_str(), mime.as_str(), bytes.as_ref() as &[u8]));

    submit_requirement(
        &state.pool,
        &state.upload_dir,
        state.max_upload_bytes,
        user.employee_id,
        requirement_id,
        note.as_deref(),
        upload_ref,
    )
    .await?;

    redirect_with_flash(
        &session,
        "/me/requirements",
        "success",
        "Requirement submitted",
    )
    .await
}

pub async fn download_my_requirement_file(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(requirement_id): Path<Uuid>,
) -> AppResult<Response> {
    let (req, bytes) = read_requirement_file(
        &state.pool,
        &state.upload_dir,
        user.employee_id,
        requirement_id,
    )
    .await?;

    Ok(requirement_file_response(
        req.file_name,
        req.file_mime,
        bytes,
    ))
}
