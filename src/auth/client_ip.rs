use std::net::SocketAddr;

use axum::{extract::ConnectInfo, http::HeaderMap};

/// Resolve the client IP for rate limiting and login lockouts.
///
/// When `trust_proxy` is enabled, prefer headers set by the edge proxy:
/// `X-Real-IP` first, then the **rightmost** hop in `X-Forwarded-For` (the TCP
/// peer seen by the proxy). Do not use the leftmost `X-Forwarded-For` value —
/// clients can spoof it before the proxy appends the real peer.
pub fn client_ip(
    trust_proxy: bool,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    headers: &HeaderMap,
) -> String {
    if trust_proxy {
        if let Some(ip) = header_ip(headers, "x-real-ip") {
            return ip;
        }
        if let Some(ip) = rightmost_forwarded_ip(headers) {
            return ip;
        }
    }

    connect_info
        .map(|info| info.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn header_ip(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_ip_token)
}

fn rightmost_forwarded_ip(headers: &HeaderMap) -> Option<String> {
    let value = headers.get("x-forwarded-for")?.to_str().ok()?;
    value.split(',').filter_map(parse_ip_token).next_back()
}

fn parse_ip_token(token: &str) -> Option<String> {
    let ip = token.trim();
    if ip.is_empty() {
        return None;
    }
    Some(ip.to_string())
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use axum::http::HeaderValue;

    use super::*;

    fn addr(port: u16) -> ConnectInfo<SocketAddr> {
        ConnectInfo(SocketAddr::from((Ipv4Addr::new(10, 0, 0, 5), port)))
    }

    #[test]
    fn ignores_forwarded_headers_when_proxy_trust_disabled() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.9, 198.51.100.2"),
        );
        headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.9"));

        let ip = client_ip(false, Some(addr(8080)), &headers);
        assert_eq!(ip, "10.0.0.5");
    }

    #[test]
    fn prefers_x_real_ip_when_trusting_proxy() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.99, 198.51.100.2"),
        );
        headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.9"));

        let ip = client_ip(true, Some(addr(8080)), &headers);
        assert_eq!(ip, "203.0.113.9");
    }

    #[test]
    fn uses_rightmost_forwarded_ip_not_spoofed_leftmost() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.99, 198.51.100.2"),
        );

        let ip = client_ip(true, Some(addr(8080)), &headers);
        assert_eq!(ip, "198.51.100.2");
    }

    #[test]
    fn single_forwarded_ip_is_used_when_trusting_proxy() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.9"));

        let ip = client_ip(true, Some(addr(8080)), &headers);
        assert_eq!(ip, "203.0.113.9");
    }
}
