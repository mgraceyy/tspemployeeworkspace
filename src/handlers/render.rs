use axum::response::{Html, IntoResponse, Redirect, Response};
use minijinja::Value;

use crate::auth::UserSession;
use crate::error::AppResult;
use crate::state::AppState;
use crate::templates::with_layout;

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
    user: Option<UserSession>,
    company_name: &str,
    title: &str,
    template: &str,
    body: Value,
) -> AppResult<HtmlPage> {
    let ctx = with_layout(company_name, user, title, body);
    let html = state.templates.render(template, ctx)?;
    Ok(HtmlPage(html))
}