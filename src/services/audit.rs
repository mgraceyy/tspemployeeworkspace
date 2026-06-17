use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Debug, sqlx::FromRow)]
pub struct AuditLogEntry {
    pub id: Uuid,
    pub actor_code: String,
    pub actor_name: String,
    pub action: String,
    pub summary: String,
    pub created_at: OffsetDateTime,
}

pub async fn log_action(
    pool: &PgPool,
    actor_id: Uuid,
    action: &str,
    summary: &str,
) -> AppResult<()> {
    sqlx::query("INSERT INTO admin_audit_logs (actor_id, action, summary) VALUES ($1, $2, $3)")
        .bind(actor_id)
        .bind(action)
        .bind(summary)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    tracing::info!(
        actor_id = %actor_id,
        action = action,
        summary = summary,
        "audit"
    );

    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct AuditLogQuery {
    pub search: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

pub async fn count_audit_logs(pool: &PgPool, search: Option<&str>) -> AppResult<i64> {
    let pattern = crate::services::pagination::search_pattern(search);
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM admin_audit_logs al
         JOIN employees a ON a.id = al.actor_id
         WHERE ($1::text IS NULL OR (
             al.action ILIKE $1
             OR al.summary ILIKE $1
             OR a.employee_code ILIKE $1
             OR a.full_name ILIKE $1
         ))",
    )
    .bind(pattern)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(count)
}

pub async fn list_audit_logs(
    pool: &PgPool,
    query: &AuditLogQuery,
) -> AppResult<Vec<AuditLogEntry>> {
    let pattern = crate::services::pagination::search_pattern(query.search.as_deref());
    let rows = sqlx::query_as::<_, AuditLogEntry>(
        "SELECT al.id,
                a.employee_code AS actor_code,
                a.full_name AS actor_name,
                al.action,
                al.summary,
                al.created_at
         FROM admin_audit_logs al
         JOIN employees a ON a.id = al.actor_id
         WHERE ($1::text IS NULL OR (
             al.action ILIKE $1
             OR al.summary ILIKE $1
             OR a.employee_code ILIKE $1
             OR a.full_name ILIKE $1
         ))
         ORDER BY al.created_at DESC
         LIMIT $2 OFFSET $3",
    )
    .bind(pattern)
    .bind(query.limit)
    .bind(query.offset)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}
