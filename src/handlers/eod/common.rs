use serde::Deserialize;

use crate::services::eod::{parse_task_lines, EodTaskInput};

#[derive(Deserialize)]
pub struct EodForm {
    pub(crate) summary: Option<String>,
    completed: Option<String>,
    pending: Option<String>,
    blocked: Option<String>,
    planned: Option<String>,
    pub(crate) action: String,
}

impl EodForm {
    pub(crate) fn is_submit(&self) -> bool {
        self.action == "submit"
    }

    pub(crate) fn summary_text(&self) -> &str {
        self.summary.as_deref().unwrap_or("")
    }
}

pub fn collect_tasks(form: &EodForm) -> Vec<EodTaskInput> {
    let mut tasks = Vec::new();
    tasks.extend(parse_task_lines(
        form.completed.as_deref().unwrap_or(""),
        crate::models::EodTaskKind::Completed,
    ));
    tasks.extend(parse_task_lines(
        form.pending.as_deref().unwrap_or(""),
        crate::models::EodTaskKind::Pending,
    ));
    tasks.extend(parse_task_lines(
        form.blocked.as_deref().unwrap_or(""),
        crate::models::EodTaskKind::Blocked,
    ));
    tasks.extend(parse_task_lines(
        form.planned.as_deref().unwrap_or(""),
        crate::models::EodTaskKind::Planned,
    ));
    tasks
}
