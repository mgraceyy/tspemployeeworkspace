use axum::response::{Html, IntoResponse, Redirect, Response};
use minijinja::Value;
use tower_sessions::Session;

use crate::auth::csrf::get_or_create_token;
use crate::auth::session::SESSION_IDLE_HOURS;
use crate::auth::{take_flash, UserSession};
use crate::error::AppResult;
use crate::services::leave::count_pending_for_manager;
use crate::services::notifications::list_for_user;
use crate::services::ot::count_pending;
use crate::services::requirements::count_pending_for_manager as count_pending_requirements;
use crate::state::AppState;
use crate::templates::{with_layout, LayoutContext};

pub struct HtmlPage(pub String);

impl IntoResponse for HtmlPage {
    fn into_response(self) -> Response {
        Html(self.0).into_response()
    }
}

pub enum PageOrRedirect {
    Page(HtmlPage),
    Redirect(Redirect),
}

impl IntoResponse for PageOrRedirect {
    fn into_response(self) -> Response {
        match self {
            PageOrRedirect::Page(page) => page.into_response(),
            PageOrRedirect::Redirect(redirect) => redirect.into_response(),
        }
    }
}

pub async fn render_page(
    state: &AppState,
    session: &Session,
    user: Option<UserSession>,
    company_name: &str,
    title: &str,
    template: &str,
    body: Value,
) -> AppResult<HtmlPage> {
    let flash = take_flash(session).await?;
    let csrf_token = get_or_create_token(session).await?;
    let (
        pending_ot_count,
        pending_leave_count,
        pending_requirements_count,
        pending_eod,
        notification_count,
    ) = if let Some(ref current_user) = user {
        let notifications = list_for_user(&state.pool, current_user).await?;
        let is_manager_or_admin = current_user.role.is_manager_or_admin();
        let is_admin = current_user.role.is_admin();
        let pending_ot_count = if is_manager_or_admin {
            count_pending(&state.pool, current_user.employee_id, is_admin).await?
        } else {
            0
        };
        let pending_leave_count = if is_manager_or_admin {
            count_pending_for_manager(&state.pool, current_user.employee_id, is_admin).await?
        } else {
            0
        };
        let pending_requirements_count = if is_manager_or_admin {
            count_pending_requirements(&state.pool, current_user.employee_id, is_admin).await?
        } else {
            0
        };
        let pending_eod = notifications.iter().any(|n| n.kind == "missing_eod");
        (
            pending_ot_count,
            pending_leave_count,
            pending_requirements_count,
            pending_eod,
            notifications.len() as i64,
        )
    } else {
        (0, 0, 0, false, 0)
    };

    let ctx = with_layout(LayoutContext {
        company_name,
        user,
        title,
        content: body,
        flash,
        pending_ot_count,
        pending_leave_count,
        pending_requirements_count,
        pending_eod,
        notification_count,
        csrf_token: &csrf_token,
        session_idle_hours: SESSION_IDLE_HOURS,
    });
    let html = state.templates.render(template, ctx)?;
    Ok(HtmlPage(html))
}
