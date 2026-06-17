use axum::{extract::State, response::Redirect, Form};
use minijinja::context;
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::AuthUser;
use crate::error::AppResult;
use crate::handlers::flash::redirect_with_flash;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    notifications::{dismiss, dismiss_all, list_for_user},
    settings::get_settings,
};
use crate::state::AppState;

pub async fn notifications_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let notifications = list_for_user(&state.pool, &user).await?;

    let rows: Vec<_> = notifications
        .iter()
        .map(|n| {
            context! {
                key => n.key.clone(),
                kind => n.kind.clone(),
                severity => n.severity.clone(),
                title => n.title.clone(),
                message => n.message.clone(),
                href => n.href.clone(),
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Notifications",
        "notifications.html",
        context! {
            notifications => rows,
            count => notifications.len(),
        },
    )
    .await
}

#[derive(Deserialize)]
pub struct DismissNotificationForm {
    key: String,
}

pub async fn dismiss_notification(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<DismissNotificationForm>,
) -> AppResult<Redirect> {
    dismiss(&state.pool, user.employee_id, &form.key).await?;
    redirect_with_flash(
        &session,
        "/notifications",
        "success",
        "Notification dismissed",
    )
    .await
}

#[derive(Deserialize)]
pub struct DismissAllForm {
    keys: Vec<String>,
}

pub async fn dismiss_all_notifications(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Form(form): Form<DismissAllForm>,
) -> AppResult<Redirect> {
    dismiss_all(&state.pool, user.employee_id, &form.keys).await?;
    redirect_with_flash(
        &session,
        "/notifications",
        "success",
        "All notifications dismissed",
    )
    .await
}
