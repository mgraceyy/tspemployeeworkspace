use axum::extract::{Query, State};
use minijinja::context;
use tower_sessions::Session;

use crate::auth::AuthUser;
use crate::error::AppResult;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    audit::{count_audit_logs, list_audit_logs, AuditLogQuery},
    pagination::{clamp_page, clamp_per_page, offset, PageInfo},
    settings::get_settings,
    timezone::format_time,
};
use crate::state::AppState;

use super::common::{pagination_context, ListPageQuery};

pub async fn audit_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Query(list_query): Query<ListPageQuery>,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let page = clamp_page(list_query.page);
    let per_page = clamp_per_page(list_query.per_page);
    let total = count_audit_logs(&state.pool, list_query.q.as_deref()).await?;
    let page_info = PageInfo::new(page, per_page, total);
    let logs = list_audit_logs(
        &state.pool,
        &AuditLogQuery {
            search: list_query.q.clone(),
            limit: per_page,
            offset: offset(page, per_page),
        },
    )
    .await?;

    let tz = settings.timezone.as_str();
    let log_rows: Vec<_> = logs
        .iter()
        .map(|log| {
            context! {
                actor_code => log.actor_code.clone(),
                actor_name => log.actor_name.clone(),
                action => log.action.clone(),
                summary => log.summary.clone(),
                created_at => format_time(log.created_at, tz),
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Admin Audit Log",
        "admin/audit.html",
        context! {
            logs => log_rows,
            pagination => pagination_context("/admin/audit", &list_query, &page_info),
        },
    )
    .await
}
