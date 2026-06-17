use std::sync::Arc;

use minijinja::{context, Environment, Error as TemplateError, Value};

use crate::auth::{FlashMessage, UserSession};
use crate::services::hours::format_minutes;

fn format_minutes_filter(value: i32) -> String {
    format_minutes(value)
}

pub struct TemplateEngine {
    env: Environment<'static>,
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateEngine {
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.set_loader(minijinja::path_loader("templates"));
        env.add_filter("format_minutes", format_minutes_filter);
        Self { env }
    }

    pub fn render(&self, name: &str, ctx: Value) -> Result<String, TemplateError> {
        let template = self.env.get_template(name)?;
        template.render(ctx)
    }
}

pub fn engine() -> Arc<TemplateEngine> {
    Arc::new(TemplateEngine::new())
}

pub struct LayoutContext<'a> {
    pub company_name: &'a str,
    pub user: Option<UserSession>,
    pub title: &'a str,
    pub content: Value,
    pub flash: Option<FlashMessage>,
    pub pending_ot_count: i64,
    pub pending_leave_count: i64,
    pub pending_requirements_count: i64,
    pub pending_eod: bool,
    pub notification_count: i64,
    pub csrf_token: &'a str,
    pub session_idle_hours: i32,
}

pub fn with_layout(ctx: LayoutContext<'_>) -> Value {
    context! {
        company_name => ctx.company_name,
        user => ctx.user,
        title => ctx.title,
        content => ctx.content,
        flash => ctx.flash,
        pending_ot_count => ctx.pending_ot_count,
        pending_leave_count => ctx.pending_leave_count,
        pending_requirements_count => ctx.pending_requirements_count,
        pending_eod => ctx.pending_eod,
        notification_count => ctx.notification_count,
        csrf_token => ctx.csrf_token,
        session_idle_hours => ctx.session_idle_hours,
    }
}
