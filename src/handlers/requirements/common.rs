use crate::models::RequirementStatus;
use crate::services::requirements::is_requirement_expired;
use axum::{
    body::Body,
    http::header,
    response::{IntoResponse, Response},
};

pub(crate) fn status_label(status: RequirementStatus) -> &'static str {
    match status {
        RequirementStatus::Missing => "Missing",
        RequirementStatus::Submitted => "Submitted",
        RequirementStatus::Approved => "Approved",
        RequirementStatus::Rejected => "Rejected",
    }
}

pub(crate) fn status_display(
    req: &crate::models::EmployeeRequirement,
) -> (&'static str, &'static str) {
    if req.status == RequirementStatus::Approved && is_requirement_expired(req.expires_at) {
        ("Expired", "expired")
    } else {
        let key = match req.status {
            RequirementStatus::Missing => "missing",
            RequirementStatus::Submitted => "submitted",
            RequirementStatus::Approved => "approved",
            RequirementStatus::Rejected => "rejected",
        };
        (status_label(req.status), key)
    }
}

pub(crate) fn format_file_size(size: Option<i64>) -> String {
    match size {
        Some(bytes) if bytes >= 1_048_576 => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
        Some(bytes) if bytes >= 1024 => format!("{:.1} KB", bytes as f64 / 1024.0),
        Some(bytes) => format!("{bytes} B"),
        None => String::new(),
    }
}

pub(crate) fn requirement_file_response(
    file_name: Option<String>,
    file_mime: Option<String>,
    bytes: Vec<u8>,
) -> Response {
    let file_name = file_name.unwrap_or_else(|| "requirement-file".to_string());
    let mime = file_mime.unwrap_or_else(|| "application/octet-stream".to_string());
    let disposition = format!("attachment; filename=\"{file_name}\"");
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        Body::from(bytes),
    )
        .into_response()
}
