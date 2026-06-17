use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{EodTask, EodTaskKind};

pub struct EodTaskInput {
    pub kind: EodTaskKind,
    pub title: String,
}

pub async fn list_tasks(pool: &PgPool, report_id: Uuid) -> AppResult<Vec<EodTask>> {
    let rows = sqlx::query_as::<_, EodTask>(
        "SELECT id, eod_report_id, kind, title, description, sort_order
         FROM eod_tasks
         WHERE eod_report_id = $1
         ORDER BY kind, sort_order",
    )
    .bind(report_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    Ok(rows)
}

pub fn tasks_to_textareas(tasks: &[EodTask]) -> (String, String, String, String) {
    let mut completed = Vec::new();
    let mut pending = Vec::new();
    let mut blocked = Vec::new();
    let mut planned = Vec::new();

    for task in tasks {
        match task.kind {
            EodTaskKind::Completed => completed.push(task.title.as_str()),
            EodTaskKind::Pending => pending.push(task.title.as_str()),
            EodTaskKind::Blocked => blocked.push(task.title.as_str()),
            EodTaskKind::Planned => planned.push(task.title.as_str()),
        }
    }

    (
        completed.join("\n"),
        pending.join("\n"),
        blocked.join("\n"),
        planned.join("\n"),
    )
}

pub fn parse_task_lines(text: &str, kind: EodTaskKind) -> Vec<EodTaskInput> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| EodTaskInput {
            kind,
            title: line.to_string(),
        })
        .collect()
}
