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
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    audit::log_action,
    payroll::deductions::{
        create_deduction_type, list_all_deduction_types, set_deduction_type_active,
    },
    settings::get_settings,
};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct DeductionTypeForm {
    code: String,
    name: String,
}

pub async fn deduction_types_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let types = list_all_deduction_types(&state.pool).await?;
    let rows: Vec<_> = types
        .iter()
        .map(|t| {
            context! {
                id => t.id,
                code => t.code.clone(),
                name => t.name.clone(),
                is_active => t.is_active,
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Deduction Types",
        "admin/deduction_types.html",
        context! { types => rows },
    )
    .await
}

pub async fn create_deduction_type_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<DeductionTypeForm>,
) -> AppResult<Redirect> {
    let created = create_deduction_type(&state.pool, &form.code, &form.name).await?;
    log_action(
        &state.pool,
        user.employee_id,
        "deduction_type.created",
        &format!("Created deduction type {} ({})", created.code, created.name),
    )
    .await?;
    redirect_with_flash(
        &session,
        "/admin/deduction-types",
        "success",
        "Deduction type created",
    )
    .await
}

pub async fn toggle_deduction_type_action(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Path(type_id): Path<Uuid>,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> AppResult<Redirect> {
    let activate = form.get("activate").map(|s| s == "true").unwrap_or(false);
    set_deduction_type_active(&state.pool, type_id, activate).await?;
    log_action(
        &state.pool,
        user.employee_id,
        if activate {
            "deduction_type.activated"
        } else {
            "deduction_type.deactivated"
        },
        &format!("Deduction type {type_id} set active={activate}"),
    )
    .await?;
    redirect_with_flash(
        &session,
        "/admin/deduction-types",
        "success",
        if activate {
            "Deduction type activated"
        } else {
            "Deduction type deactivated"
        },
    )
    .await
}
