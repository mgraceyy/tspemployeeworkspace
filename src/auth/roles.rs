use axum::http::{Method, Uri};

use crate::models::UserRole;

pub const ADMIN_HOME: &str = "/manager";

pub fn home_path_for_role(role: UserRole) -> &'static str {
    match role {
        UserRole::Admin | UserRole::Manager => ADMIN_HOME,
        _ => "/",
    }
}

/// Paths admins must not use (clock in/out and employee self-service).
pub fn admin_blocked_employee_path(path: &str) -> bool {
    if path == "/" || path.starts_with("/clock/") {
        return true;
    }
    if path.starts_with("/me/") && !path.starts_with("/me/profile") {
        return true;
    }
    false
}

pub fn admin_blocked_request(method: &Method, uri: &Uri) -> bool {
    let _ = method;
    admin_blocked_employee_path(uri.path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Uri;

    #[test]
    fn admin_home_is_dashboard() {
        assert_eq!(home_path_for_role(UserRole::Admin), "/manager");
        assert_eq!(home_path_for_role(UserRole::Employee), "/");
        assert_eq!(home_path_for_role(UserRole::Manager), "/manager");
    }

    #[test]
    fn admin_blocked_paths() {
        assert!(admin_blocked_employee_path("/"));
        assert!(admin_blocked_employee_path("/clock/in"));
        assert!(admin_blocked_employee_path("/me/timesheet"));
        assert!(admin_blocked_employee_path("/me/eod"));
        assert!(!admin_blocked_employee_path("/me/profile"));
        assert!(!admin_blocked_employee_path("/me/profile/photo"));
        assert!(!admin_blocked_employee_path("/notifications"));
        assert!(!admin_blocked_employee_path("/admin/employees"));
    }

    #[test]
    fn admin_blocked_request_uses_path() {
        let uri: Uri = "/clock/out".parse().unwrap();
        assert!(admin_blocked_request(&Method::POST, &uri));
    }
}