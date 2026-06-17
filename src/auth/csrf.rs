use axum::{
    body::Body,
    extract::Request,
    http::{header, Method},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::services::uploads::DEFAULT_MAX_UPLOAD_BYTES;
use tower_sessions::Session;

use crate::error::AppError;

pub const CSRF_SESSION_KEY: &str = "csrf_token";

pub async fn get_or_create_token(session: &Session) -> crate::error::AppResult<String> {
    if let Some(token) = session
        .get::<String>(CSRF_SESSION_KEY)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
    {
        return Ok(token);
    }

    let token = uuid::Uuid::new_v4().to_string();
    session
        .insert(CSRF_SESSION_KEY, token.clone())
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(token)
}

pub async fn validate_token(
    session: &Session,
    submitted: Option<&str>,
) -> crate::error::AppResult<()> {
    let expected = session
        .get::<String>(CSRF_SESSION_KEY)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| {
            AppError::bad_request("Missing CSRF token — refresh the page and try again")
        })?;

    let submitted = submitted.filter(|value| !value.is_empty()).ok_or_else(|| {
        AppError::bad_request("Missing CSRF token — refresh the page and try again")
    })?;

    if !constant_time_eq(submitted, &expected) {
        return Err(AppError::bad_request(
            "Invalid CSRF token — refresh the page and try again",
        ));
    }
    Ok(())
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.bytes().zip(right.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn max_post_body_bytes(content_type: Option<&header::HeaderValue>) -> usize {
    let is_multipart = content_type
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("multipart/form-data"));
    if is_multipart {
        DEFAULT_MAX_UPLOAD_BYTES + 2 * 1024 * 1024
    } else {
        2 * 1024 * 1024
    }
}

fn multipart_boundary(content_type: &str) -> Option<String> {
    content_type
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("boundary="))
        .map(|value| value.trim_matches('"').to_string())
}

fn extract_csrf_from_multipart(body: &[u8], boundary: &str) -> Option<String> {
    let marker = b"name=\"csrf_token\"";
    let pos = body
        .windows(marker.len())
        .position(|window| window == marker)?;
    let after_marker = &body[pos..];
    let header_end = after_marker
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?;
    let value_start = pos + header_end + 4;
    let value_region = &body[value_start..];
    let boundary_suffix = format!("\r\n--{boundary}");
    let end = value_region
        .windows(boundary_suffix.len())
        .position(|window| window == boundary_suffix.as_bytes())
        .or_else(|| value_region.windows(2).position(|window| window == b"\r\n"))?;
    String::from_utf8(value_region[..end].to_vec()).ok()
}

fn extract_csrf_from_body(
    content_type: Option<&header::HeaderValue>,
    body: &[u8],
) -> Option<String> {
    let content_type = content_type.and_then(|value| value.to_str().ok())?;
    if content_type.starts_with("application/x-www-form-urlencoded") {
        return extract_csrf_from_form(&String::from_utf8_lossy(body));
    }
    if content_type.starts_with("multipart/form-data") {
        let boundary = multipart_boundary(content_type)?;
        return extract_csrf_from_multipart(body, &boundary);
    }
    None
}

fn extract_csrf_from_form(body: &str) -> Option<String> {
    for pair in body.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next()? == "csrf_token" {
            let value = parts.next()?;
            return Some(decode_form_value(value));
        }
    }
    None
}

fn decode_form_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            let hi = chars.next();
            let lo = chars.next();
            if let (Some(h), Some(l)) = (hi, lo) {
                let hex = format!("{h}{l}");
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    out.push(byte as char);
                    continue;
                }
            }
            out.push('%');
            if let Some(h) = hi {
                out.push(h);
            }
            if let Some(l) = lo {
                out.push(l);
            }
        } else if ch == '+' {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

pub async fn validate_post(session: Session, request: Request, next: Next) -> Response {
    if request.method() != Method::POST {
        return next.run(request).await;
    }

    let path = request.uri().path();
    if path == "/health" {
        return next.run(request).await;
    }

    let (parts, body) = request.into_parts();
    let limit = max_post_body_bytes(parts.headers.get(header::CONTENT_TYPE));
    let Ok(bytes) = axum::body::to_bytes(body, limit).await else {
        return AppError::bad_request("Request body too large").into_response();
    };

    let submitted = extract_csrf_from_body(parts.headers.get(header::CONTENT_TYPE), &bytes);
    if let Err(error) = validate_token(&session, submitted.as_deref()).await {
        return error.into_response();
    }

    let body = Body::from(bytes);
    let request = Request::from_parts(parts, body);
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_csrf_from_urlencoded_body() {
        let body = "employee_code=ADMIN&pin=1234&csrf_token=abc-def";
        assert_eq!(extract_csrf_from_form(body).as_deref(), Some("abc-def"));
    }

    #[test]
    fn decode_form_value_handles_spaces_and_percent_encoding() {
        assert_eq!(decode_form_value("hello+world"), "hello world");
        assert_eq!(decode_form_value("token%2Dvalue"), "token-value");
    }
}
