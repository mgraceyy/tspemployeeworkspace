use minijinja::context;

use crate::models::RequirementStatus;
use crate::services::requirements::{has_uploaded_file, is_requirement_expired};
use crate::services::timezone::format_time;
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

pub(crate) fn build_requirement_rows(
    reqs: &[crate::models::EmployeeRequirement],
    timezone: &str,
) -> Vec<minijinja::value::Value> {
    reqs.iter()
        .map(|r| {
            let (status, _) = status_display(r);
            context! {
                id => r.id,
                name => r.type_name.clone(),
                description => r.type_description.clone(),
                status => status,
                employee_note => r.employee_note.clone().unwrap_or_default(),
                admin_note => r.admin_note.clone().unwrap_or_default(),
                submitted_at => r.submitted_at.map(|dt| format_time(dt, timezone)).unwrap_or_default(),
                expires_at => r.expires_at.map(|dt| format_time(dt, timezone)).unwrap_or_default(),
                is_expired => is_requirement_expired(r.expires_at),
                can_review => r.status == RequirementStatus::Submitted,
                has_file => has_uploaded_file(r),
                file_name => r.file_name.clone().unwrap_or_default(),
                file_size => format_file_size(r.file_size),
            }
        })
        .collect()
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
