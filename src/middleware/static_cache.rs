use axum::{
    extract::Request,
    http::{header, HeaderValue},
    middleware::Next,
    response::Response,
};

fn static_cache_control() -> &'static str {
    let production = std::env::var("APP_ENV")
        .map(|v| v.eq_ignore_ascii_case("production"))
        .unwrap_or(false);
    if production {
        "public, max-age=86400"
    } else {
        "no-cache"
    }
}

pub async fn add_static_cache_headers(request: Request, next: Next) -> Response {
    let is_static = request.uri().path().starts_with("/static/");
    let mut response = next.run(request).await;
    if is_static && response.status().is_success() {
        if let Ok(value) = HeaderValue::from_str(static_cache_control()) {
            response.headers_mut().insert(header::CACHE_CONTROL, value);
        }
    }
    response
}
