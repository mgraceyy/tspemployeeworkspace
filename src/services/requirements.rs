use std::path::Path;

use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{EmployeeRequirement, RequirementStatus, RequirementType};
use crate::services::uploads::{delete_stored_file, store_requirement_file};

pub fn is_requirement_expired(expires_at: Option<OffsetDateTime>) -> bool {
    expires_at.is_some_and(|t| t < OffsetDateTime::now_utc())
}

pub async fn list_types(pool: &PgPool) -> AppResult<Vec<RequirementType>> {
    let rows = sqlx::query_as::<_, RequirementType>(
        "SELECT id, name, description, is_required, requires_upload, is_active, sort_order,
                expires_after_days, created_at
         FROM requirement_types
         ORDER BY sort_order, name",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn create_type(
    pool: &PgPool,
    name: &str,
    description: &str,
    is_required: bool,
    requires_upload: bool,
    sort_order: i32,
    expires_after_days: Option<i32>,
) -> AppResult<RequirementType> {
    if let Some(days) = expires_after_days {
        if days <= 0 {
            return Err(AppError::bad_request(
                "Expiry days must be greater than zero",
            ));
        }
    }

    let row = sqlx::query_as::<_, RequirementType>(
        "INSERT INTO requirement_types (name, description, is_required, requires_upload, sort_order, expires_after_days)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, name, description, is_required, requires_upload, is_active, sort_order,
                   expires_after_days, created_at",
    )
    .bind(name.trim())
    .bind(description.trim())
    .bind(is_required)
    .bind(requires_upload)
    .bind(sort_order)
    .bind(expires_after_days)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(row)
}

pub struct RequirementTypeUpdate<'a> {
    pub type_id: Uuid,
    pub name: &'a str,
    pub description: &'a str,
    pub is_required: bool,
    pub requires_upload: bool,
    pub is_active: bool,
    pub sort_order: i32,
    pub expires_after_days: Option<i32>,
}

pub async fn update_type(
    pool: &PgPool,
    update: &RequirementTypeUpdate<'_>,
) -> AppResult<RequirementType> {
    if let Some(days) = update.expires_after_days {
        if days <= 0 {
            return Err(AppError::bad_request(
                "Expiry days must be greater than zero",
            ));
        }
    }

    let row = sqlx::query_as::<_, RequirementType>(
        "UPDATE requirement_types
         SET name = $2, description = $3, is_required = $4, requires_upload = $5, is_active = $6,
             sort_order = $7, expires_after_days = $8
         WHERE id = $1
         RETURNING id, name, description, is_required, requires_upload, is_active, sort_order,
                   expires_after_days, created_at",
    )
    .bind(update.type_id)
    .bind(update.name.trim())
    .bind(update.description.trim())
    .bind(update.is_required)
    .bind(update.requires_upload)
    .bind(update.is_active)
    .bind(update.sort_order)
    .bind(update.expires_after_days)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::NotFound)?;
    Ok(row)
}

pub async fn seed_for_employee(pool: &PgPool, employee_id: Uuid) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO employee_requirements (employee_id, requirement_type_id, status)
         SELECT $1, rt.id, 'missing'
         FROM requirement_types rt
         WHERE rt.is_active = TRUE
         ON CONFLICT (employee_id, requirement_type_id) DO NOTHING",
    )
    .bind(employee_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

pub async fn seed_new_type_for_all_employees(pool: &PgPool, type_id: Uuid) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO employee_requirements (employee_id, requirement_type_id, status)
         SELECT e.id, $1, 'missing'
         FROM employees e
         WHERE e.is_active = TRUE
         ON CONFLICT (employee_id, requirement_type_id) DO NOTHING",
    )
    .bind(type_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

const EMPLOYEE_REQUIREMENT_SELECT: &str = "SELECT er.id, er.employee_id, er.requirement_type_id,
                rt.name AS type_name, rt.description AS type_description, rt.is_required,
                rt.requires_upload, er.status, er.employee_note, er.admin_note, er.submitted_at,
                er.expires_at, er.file_name, er.file_stored_path, er.file_mime, er.file_size";

pub async fn list_for_employee(
    pool: &PgPool,
    employee_id: Uuid,
) -> AppResult<Vec<EmployeeRequirement>> {
    seed_for_employee(pool, employee_id).await?;
    let query = format!(
        "{EMPLOYEE_REQUIREMENT_SELECT}
         FROM employee_requirements er
         JOIN requirement_types rt ON rt.id = er.requirement_type_id
         WHERE er.employee_id = $1 AND rt.is_active = TRUE
         ORDER BY rt.sort_order, rt.name"
    );
    let rows = sqlx::query_as::<_, EmployeeRequirement>(&query)
        .bind(employee_id)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn get_employee_requirement(
    pool: &PgPool,
    employee_id: Uuid,
    requirement_id: Uuid,
) -> AppResult<EmployeeRequirement> {
    let query = format!(
        "{EMPLOYEE_REQUIREMENT_SELECT}
         FROM employee_requirements er
         JOIN requirement_types rt ON rt.id = er.requirement_type_id
         WHERE er.id = $1 AND er.employee_id = $2"
    );
    sqlx::query_as::<_, EmployeeRequirement>(&query)
        .bind(requirement_id)
        .bind(employee_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or(AppError::NotFound)
}

pub async fn submit_requirement(
    pool: &PgPool,
    upload_dir: &Path,
    max_upload_bytes: usize,
    employee_id: Uuid,
    requirement_id: Uuid,
    note: Option<&str>,
    upload: Option<(&str, &str, &[u8])>,
) -> AppResult<()> {
    let current = get_employee_requirement(pool, employee_id, requirement_id).await?;

    if !can_submit_requirement(&current) {
        return Err(AppError::bad_request(
            "Requirement cannot be submitted in its current state",
        ));
    }

    if current.requires_upload && upload.is_none() && current.file_stored_path.is_none() {
        return Err(AppError::bad_request(
            "This requirement needs a file upload (PDF, image, or Word document)",
        ));
    }

    let stored = if let Some((original_name, mime_type, bytes)) = upload {
        if let Some(old_path) = current.file_stored_path.as_deref() {
            delete_stored_file(upload_dir, old_path).await?;
        }
        Some(
            store_requirement_file(
                upload_dir,
                employee_id,
                requirement_id,
                original_name,
                mime_type,
                bytes,
                max_upload_bytes,
            )
            .await?,
        )
    } else {
        None
    };

    let updated = if let Some(file) = stored {
        sqlx::query(
            "UPDATE employee_requirements
             SET status = 'submitted',
                 employee_note = $3,
                 submitted_at = now(),
                 expires_at = NULL,
                 file_name = $4,
                 file_stored_path = $5,
                 file_mime = $6,
                 file_size = $7
             WHERE id = $1 AND employee_id = $2
               AND (
                 status IN ('missing', 'rejected')
                 OR (status = 'approved' AND expires_at IS NOT NULL AND expires_at < now())
               )",
        )
        .bind(requirement_id)
        .bind(employee_id)
        .bind(note.map(str::trim).filter(|n| !n.is_empty()))
        .bind(&file.original_name)
        .bind(&file.stored_path)
        .bind(&file.mime_type)
        .bind(file.size_bytes)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
    } else {
        sqlx::query(
            "UPDATE employee_requirements
             SET status = 'submitted',
                 employee_note = $3,
                 submitted_at = now(),
                 expires_at = NULL
             WHERE id = $1 AND employee_id = $2
               AND (
                 status IN ('missing', 'rejected')
                 OR (status = 'approved' AND expires_at IS NOT NULL AND expires_at < now())
               )",
        )
        .bind(requirement_id)
        .bind(employee_id)
        .bind(note.map(str::trim).filter(|n| !n.is_empty()))
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
    };

    if updated.rows_affected() == 0 {
        return Err(AppError::bad_request(
            "Requirement cannot be submitted in its current state",
        ));
    }
    Ok(())
}

pub async fn review_requirement(
    pool: &PgPool,
    employee_id: Uuid,
    requirement_id: Uuid,
    reviewer_id: Uuid,
    approve: bool,
    note: Option<&str>,
) -> AppResult<()> {
    let expires_after_days: Option<i32> = if approve {
        sqlx::query_scalar(
            "SELECT rt.expires_after_days
             FROM employee_requirements er
             JOIN requirement_types rt ON rt.id = er.requirement_type_id
             WHERE er.id = $1 AND er.employee_id = $2 AND er.status = 'submitted'",
        )
        .bind(requirement_id)
        .bind(employee_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or(AppError::bad_request("Requirement is not pending review"))?
    } else {
        None
    };

    let status = if approve {
        RequirementStatus::Approved
    } else {
        RequirementStatus::Rejected
    };

    let expires_at = expires_after_days
        .map(|days| OffsetDateTime::now_utc() + time::Duration::days(days as i64));

    let updated = sqlx::query(
        "UPDATE employee_requirements
         SET status = $3,
             admin_note = $4,
             reviewed_by = $5,
             reviewed_at = now(),
             expires_at = $6
         WHERE id = $1 AND employee_id = $2 AND status = 'submitted'",
    )
    .bind(requirement_id)
    .bind(employee_id)
    .bind(status)
    .bind(note.map(str::trim).filter(|n| !n.is_empty()))
    .bind(reviewer_id)
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if updated.rows_affected() == 0 {
        return Err(AppError::bad_request("Requirement is not pending review"));
    }
    Ok(())
}

pub fn can_submit_requirement(req: &EmployeeRequirement) -> bool {
    matches!(
        req.status,
        RequirementStatus::Missing | RequirementStatus::Rejected
    ) || (req.status == RequirementStatus::Approved && is_requirement_expired(req.expires_at))
}

pub fn has_uploaded_file(req: &EmployeeRequirement) -> bool {
    req.file_stored_path.is_some()
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PendingRequirementReview {
    pub requirement_id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub full_name: String,
    pub type_name: String,
    pub submitted_at: Option<OffsetDateTime>,
}

pub async fn count_pending_for_manager(
    pool: &PgPool,
    manager_id: Uuid,
    is_admin: bool,
) -> AppResult<i64> {
    let count: i64 = if is_admin {
        sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM employee_requirements
             WHERE status = 'submitted'",
        )
        .fetch_one(pool)
        .await
    } else {
        sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM employee_requirements er
             JOIN employees e ON e.id = er.employee_id
             WHERE er.status = 'submitted' AND e.manager_id = $1",
        )
        .bind(manager_id)
        .fetch_one(pool)
        .await
    }
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(count)
}

pub async fn list_pending_for_manager(
    pool: &PgPool,
    manager_id: Uuid,
    is_admin: bool,
) -> AppResult<Vec<PendingRequirementReview>> {
    let rows = if is_admin {
        sqlx::query_as::<_, PendingRequirementReview>(
            "SELECT er.id AS requirement_id, er.employee_id, e.employee_code, e.full_name,
                    rt.name AS type_name, er.submitted_at
             FROM employee_requirements er
             JOIN employees e ON e.id = er.employee_id
             JOIN requirement_types rt ON rt.id = er.requirement_type_id
             WHERE er.status = 'submitted'
             ORDER BY er.submitted_at, e.full_name",
        )
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, PendingRequirementReview>(
            "SELECT er.id AS requirement_id, er.employee_id, e.employee_code, e.full_name,
                    rt.name AS type_name, er.submitted_at
             FROM employee_requirements er
             JOIN employees e ON e.id = er.employee_id
             JOIN requirement_types rt ON rt.id = er.requirement_type_id
             WHERE er.status = 'submitted' AND e.manager_id = $1
             ORDER BY er.submitted_at, e.full_name",
        )
        .bind(manager_id)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub async fn read_requirement_file(
    pool: &PgPool,
    upload_dir: &Path,
    employee_id: Uuid,
    requirement_id: Uuid,
) -> AppResult<(EmployeeRequirement, Vec<u8>)> {
    let req = get_employee_requirement(pool, employee_id, requirement_id).await?;
    let stored_path = req.file_stored_path.as_deref().ok_or(AppError::NotFound)?;
    let bytes = crate::services::uploads::read_stored_file(upload_dir, stored_path).await?;
    Ok((req, bytes))
}
