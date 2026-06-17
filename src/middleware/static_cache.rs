use axum::{
    extract::Request,
    http::{header, HeaderValue},
    middleware::Next,
    response::Response,
};

pub async fn add_static_cache_headers(request: Request, next: Next) -> Response {
    let is_static = request.uri().path().starts_with("/static/");
    let mut response = next.run(request).await;
    if is_static && response.status().is_success() {
        if let Ok(value) = HeaderValue::from_str("public, max-age=86400") {
            response.headers_mut().insert(header::CACHE_CONTROL, value);
        }
    }
    response
}
