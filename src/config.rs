use anyhow::{Context, Result};

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub session_secret: String,
    pub seed_default_admin: bool,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let seed_default_admin = std::env::var("SEED_DEFAULT_ADMIN")
            .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);

        Ok(Self {
            database_url: std::env::var("DATABASE_URL")
                .context("DATABASE_URL must be set")?,
            session_secret: std::env::var("SESSION_SECRET")
                .context("SESSION_SECRET must be set")?,
            seed_default_admin,
        })
    }
}