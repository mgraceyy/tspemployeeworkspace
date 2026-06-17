use std::sync::Arc;

use minijinja::{context, Environment, Error as TemplateError, Value};

use crate::services::hours::format_minutes;

fn format_minutes_filter(value: i32) -> String {
    format_minutes(value)
}

pub struct TemplateEngine {
    env: Environment<'static>,
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

pub fn with_layout(
    company_name: &str,
    user: Option<crate::auth::UserSession>,
    title: &str,
    content: Value,
) -> Value {
    context! {
        company_name => company_name,
        user => user,
        title => title,
        content => content,
    }
    .into()
}