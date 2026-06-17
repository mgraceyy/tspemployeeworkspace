use minijinja::context;
use serde::Deserialize;
use time::Time;

use crate::error::{AppError, AppResult};
use crate::services::pagination::PageInfo;

#[derive(Deserialize, Default)]
pub struct ListPageQuery {
    pub q: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

pub(crate) fn pagination_context(
    base_path: &str,
    query: &ListPageQuery,
    page_info: &PageInfo,
) -> minijinja::Value {
    let mut params: Vec<String> = Vec::new();
    if let Some(q) = query.q.as_deref().filter(|s| !s.trim().is_empty()) {
        params.push(format!("q={}", urlencoding_query(q)));
    }
    if query.per_page.is_some() {
        params.push(format!("per_page={}", page_info.per_page));
    }
    let base_query = if params.is_empty() {
        String::new()
    } else {
        format!("?{}", params.join("&"))
    };

    let page_link = |page: i64| -> String {
        let mut link_params = params.clone();
        link_params.push(format!("page={page}"));
        format!("{base_path}?{}", link_params.join("&"))
    };

    context! {
        page => page_info.page,
        per_page => page_info.per_page,
        total => page_info.total,
        total_pages => page_info.total_pages,
        has_prev => page_info.has_prev,
        has_next => page_info.has_next,
        prev_url => if page_info.has_prev { page_link(page_info.page - 1) } else { String::new() },
        next_url => if page_info.has_next { page_link(page_info.page + 1) } else { String::new() },
        search_query => query.q.clone().unwrap_or_default(),
        base_query => base_query,
    }
}

pub(crate) fn urlencoding_query(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            ' ' => "%20".to_string(),
            '&' => "%26".to_string(),
            '=' => "%3D".to_string(),
            '+' => "%2B".to_string(),
            c if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') => c.to_string(),
            c => format!("%{:02X}", c as u32),
        })
        .collect()
}

pub(crate) fn parse_time(value: &str) -> AppResult<Time> {
    let trimmed = value.trim();
    let parts: Vec<_> = trimmed.split(':').collect();
    if parts.len() != 2 {
        return Err(AppError::bad_request("Time must be HH:MM"));
    }
    let hour: u8 = parts[0]
        .parse()
        .map_err(|_| AppError::bad_request("Invalid hour"))?;
    let minute: u8 = parts[1]
        .parse()
        .map_err(|_| AppError::bad_request("Invalid minute"))?;
    Time::from_hms(hour, minute, 0).map_err(|_| AppError::bad_request("Invalid time"))
}
