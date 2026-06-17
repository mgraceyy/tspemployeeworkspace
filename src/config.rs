use anyhow::{Context, Result};

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub session_secret: String,
    pub seed_default_admin: bool,
    pub session_secure: bool,
    pub trust_proxy_headers: bool,
    pub shared_rate_limits: bool,
    pub upload_dir: String,
    pub max_upload_bytes: usize,
    pub bind_addr: String,
    pub port: u16,
    pub database_max_connections: u32,
    pub log_json: bool,
    pub metrics_token: Option<String>,
    pub seed_e2e_fixtures: bool,
}

fn env_is_truthy(key: &str) -> Option<bool> {
    std::env::var(key)
        .ok()
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
}

fn is_production(app_env: Option<&str>) -> bool {
    app_env
        .map(|v| v.eq_ignore_ascii_case("production"))
        .unwrap_or(false)
}

fn validate_seed_default_admin(app_env: Option<&str>, seed_default_admin: bool) -> Result<()> {
    if is_production(app_env) && seed_default_admin {
        anyhow::bail!(
            "SEED_DEFAULT_ADMIN must not be enabled when APP_ENV=production — provision admin accounts through your normal onboarding process"
        );
    }
    Ok(())
}

fn validate_seed_e2e_fixtures(app_env: Option<&str>, seed_e2e_fixtures: bool) -> Result<()> {
    if is_production(app_env) && seed_e2e_fixtures {
        anyhow::bail!(
            "SEED_E2E_FIXTURES must not be enabled when APP_ENV=production — known test accounts must never be seeded in production"
        );
    }
    Ok(())
}

const WEAK_SESSION_SECRETS: &[&str] = &[
    "change-me",
    "changeme",
    "secret",
    "session-secret",
    "your-secret-here",
];

impl Config {
    pub fn warn_if_insecure(&self) {
        let secret_lower = self.session_secret.to_lowercase();
        if self.session_secret.len() < 64 {
            tracing::warn!(
                "SESSION_SECRET is shorter than 64 characters — use a long random value in production"
            );
        }
        if WEAK_SESSION_SECRETS
            .iter()
            .any(|weak| secret_lower.contains(weak))
        {
            tracing::warn!(
                "SESSION_SECRET looks like a placeholder — generate a unique random value for production"
            );
        }
        if self.seed_default_admin {
            tracing::warn!(
                "SEED_DEFAULT_ADMIN is enabled — disable in production and provision real admin accounts"
            );
        }
    }

    pub fn from_env() -> Result<Self> {
        let app_env = std::env::var("APP_ENV").ok();
        let seed_default_admin = env_is_truthy("SEED_DEFAULT_ADMIN").unwrap_or(false);
        let seed_e2e_fixtures = env_is_truthy("SEED_E2E_FIXTURES").unwrap_or(false);
        validate_seed_default_admin(app_env.as_deref(), seed_default_admin)?;
        validate_seed_e2e_fixtures(app_env.as_deref(), seed_e2e_fixtures)?;

        let session_secure = match env_is_truthy("SESSION_SECURE_COOKIES") {
            Some(value) => value,
            None => std::env::var("APP_ENV")
                .map(|v| v.eq_ignore_ascii_case("production"))
                .unwrap_or(false),
        };

        let trust_proxy_headers = env_is_truthy("TRUST_PROXY_HEADERS").unwrap_or(false);
        let shared_rate_limits = env_is_truthy("SHARED_RATE_LIMITS").unwrap_or(false);
        let upload_dir = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "./uploads".to_string());
        let max_upload_bytes = std::env::var("MAX_UPLOAD_BYTES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(crate::services::uploads::DEFAULT_MAX_UPLOAD_BYTES);

        let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = std::env::var("PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(8080);
        let database_max_connections = std::env::var("DATABASE_MAX_CONNECTIONS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(5);
        let log_json = std::env::var("LOG_FORMAT")
            .map(|value| value.eq_ignore_ascii_case("json"))
            .unwrap_or(false);
        let metrics_token = std::env::var("METRICS_TOKEN")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        Ok(Self {
            database_url: std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?,
            session_secret: std::env::var("SESSION_SECRET")
                .context("SESSION_SECRET must be set")?,
            seed_default_admin,
            session_secure,
            trust_proxy_headers,
            shared_rate_limits,
            upload_dir,
            max_upload_bytes,
            bind_addr,
            port,
            database_max_connections,
            log_json,
            metrics_token,
            seed_e2e_fixtures,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_seed_default_admin_in_production() {
        assert!(validate_seed_default_admin(Some("production"), true).is_err());
        assert!(validate_seed_default_admin(Some("Production"), true).is_err());
    }

    #[test]
    fn allows_seed_default_admin_outside_production() {
        assert!(validate_seed_default_admin(Some("development"), true).is_ok());
        assert!(validate_seed_default_admin(None, true).is_ok());
    }

    #[test]
    fn allows_disabled_seed_in_production() {
        assert!(validate_seed_default_admin(Some("production"), false).is_ok());
    }

    #[test]
    fn rejects_seed_e2e_fixtures_in_production() {
        assert!(validate_seed_e2e_fixtures(Some("production"), true).is_err());
    }

    #[test]
    fn allows_seed_e2e_fixtures_outside_production() {
        assert!(validate_seed_e2e_fixtures(Some("development"), true).is_ok());
    }
}
