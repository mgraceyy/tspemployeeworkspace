use axum::extract::{Query, State};
use minijinja::context;
use tower_sessions::Session;

use crate::auth::AuthUser;
use crate::error::AppResult;
use crate::handlers::render::{render_page, HtmlPage};
use crate::services::{
    corrections::{count_correction_logs, list_correction_logs, CorrectionLogQuery},
    pagination::{clamp_page, clamp_per_page, offset, PageInfo},
    settings::get_settings,
    timezone::{format_date, format_time},
};
use crate::state::AppState;

use super::common::{pagination_context, ListPageQuery};

pub async fn corrections_page(
    State(state): State<AppState>,
    session: Session,
    AuthUser(user): AuthUser,
    Query(list_query): Query<ListPageQuery>,
) -> AppResult<HtmlPage> {
    let settings = get_settings(&state.pool).await?;
    let page = clamp_page(list_query.page);
    let per_page = clamp_per_page(list_query.per_page);
    let total = count_correction_logs(&state.pool, list_query.q.as_deref()).await?;
    let page_info = PageInfo::new(page, per_page, total);
    let logs = list_correction_logs(
        &state.pool,
        &CorrectionLogQuery {
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
                employee_code => log.employee_code.clone(),
                employee_name => log.employee_name.clone(),
                work_date => format_date(log.work_date),
                editor_name => log.editor_name.clone(),
                reason => log.reason.clone(),
                old_clock_in => log.old_clock_in.map(|dt| format_time(dt, tz)),
                old_clock_out => log.old_clock_out.map(|dt| format_time(dt, tz)),
                new_clock_in => log.new_clock_in.map(|dt| format_time(dt, tz)),
                new_clock_out => log.new_clock_out.map(|dt| format_time(dt, tz)),
                created_at => format_time(log.created_at, tz),
            }
        })
        .collect();

    render_page(
        &state,
        &session,
        Some(user),
        &settings.company_name,
        "Correction Audit Log",
        "admin/corrections.html",
        context! {
            logs => log_rows,
            pagination => pagination_context("/admin/corrections", &list_query, &page_info),
        },
    )
    .await
}
