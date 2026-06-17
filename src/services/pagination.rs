pub const DEFAULT_PER_PAGE: i64 = 25;
pub const MAX_PER_PAGE: i64 = 100;

pub fn clamp_page(page: Option<i64>) -> i64 {
    page.filter(|&p| p > 0).unwrap_or(1)
}

pub fn clamp_per_page(per_page: Option<i64>) -> i64 {
    per_page
        .filter(|&p| p > 0)
        .map(|p| p.min(MAX_PER_PAGE))
        .unwrap_or(DEFAULT_PER_PAGE)
}

pub fn offset(page: i64, per_page: i64) -> i64 {
    (page - 1) * per_page
}

#[derive(Debug, Clone)]
pub struct PageInfo {
    pub page: i64,
    pub per_page: i64,
    pub total: i64,
    pub total_pages: i64,
    pub has_prev: bool,
    pub has_next: bool,
}

impl PageInfo {
    pub fn new(page: i64, per_page: i64, total: i64) -> Self {
        let total_pages = if total == 0 {
            1
        } else {
            (total + per_page - 1) / per_page
        };
        Self {
            page,
            per_page,
            total,
            total_pages,
            has_prev: page > 1,
            has_next: page < total_pages,
        }
    }
}

pub fn search_pattern(search: Option<&str>) -> Option<String> {
    let trimmed = search?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(format!("%{trimmed}%"))
    }
}
