use std::time::Duration;

use crate::auth::rate_limit_store::RateLimitStore;
use crate::error::AppResult;

const MAX_ACCOUNT_ATTEMPTS: usize = 5;
const MAX_IP_ATTEMPTS: usize = 20;
const MAX_PIN_CHANGE_ACCOUNT_ATTEMPTS: usize = 5;
const MAX_PIN_CHANGE_IP_ATTEMPTS: usize = 15;
const WINDOW: Duration = Duration::from_secs(15 * 60);

#[derive(Clone)]
pub struct LoginLimiter {
    store: RateLimitStore,
}

impl LoginLimiter {
    pub fn in_memory() -> Self {
        Self {
            store: RateLimitStore::in_memory(),
        }
    }

    pub fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            store: RateLimitStore::postgres(pool),
        }
    }

    pub async fn is_locked_account(&self, employee_code: &str) -> AppResult<bool> {
        Ok(self
            .store
            .count_recent(&account_key(employee_code), WINDOW)
            .await?
            >= MAX_ACCOUNT_ATTEMPTS)
    }

    pub async fn is_locked_ip(&self, ip: &str) -> AppResult<bool> {
        Ok(self.store.count_recent(&ip_key(ip), WINDOW).await? >= MAX_IP_ATTEMPTS)
    }

    pub async fn record_failure_account(&self, employee_code: &str) -> AppResult<()> {
        self.store.record(&account_key(employee_code), WINDOW).await
    }

    pub async fn record_failure_ip(&self, ip: &str) -> AppResult<()> {
        self.store.record(&ip_key(ip), WINDOW).await
    }

    pub async fn clear_account(&self, employee_code: &str) -> AppResult<()> {
        self.store.remove(&account_key(employee_code)).await
    }

    pub async fn is_locked_pin_change_account(&self, employee_code: &str) -> AppResult<bool> {
        Ok(self
            .store
            .count_recent(&pin_account_key(employee_code), WINDOW)
            .await?
            >= MAX_PIN_CHANGE_ACCOUNT_ATTEMPTS)
    }

    pub async fn is_locked_pin_change_ip(&self, ip: &str) -> AppResult<bool> {
        Ok(self.store.count_recent(&pin_ip_key(ip), WINDOW).await? >= MAX_PIN_CHANGE_IP_ATTEMPTS)
    }

    pub async fn record_pin_change_failure_account(&self, employee_code: &str) -> AppResult<()> {
        self.store
            .record(&pin_account_key(employee_code), WINDOW)
            .await
    }

    pub async fn record_pin_change_failure_ip(&self, ip: &str) -> AppResult<()> {
        self.store.record(&pin_ip_key(ip), WINDOW).await
    }

    pub async fn clear_pin_change_account(&self, employee_code: &str) -> AppResult<()> {
        self.store.remove(&pin_account_key(employee_code)).await
    }
}

fn account_key(employee_code: &str) -> String {
    format!("acct:{employee_code}")
}

fn ip_key(ip: &str) -> String {
    format!("ip:{ip}")
}

fn pin_account_key(employee_code: &str) -> String {
    format!("pin_acct:{employee_code}")
}

fn pin_ip_key(ip: &str) -> String {
    format!("pin_ip:{ip}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn locks_account_after_max_failures() {
        let limiter = LoginLimiter::in_memory();
        for _ in 0..MAX_ACCOUNT_ATTEMPTS {
            limiter.record_failure_account("EMP001").await.unwrap();
        }
        assert!(limiter.is_locked_account("EMP001").await.unwrap());
    }

    #[tokio::test]
    async fn locks_ip_after_max_failures() {
        let limiter = LoginLimiter::in_memory();
        for _ in 0..MAX_IP_ATTEMPTS {
            limiter.record_failure_ip("203.0.113.10").await.unwrap();
        }
        assert!(limiter.is_locked_ip("203.0.113.10").await.unwrap());
    }

    #[tokio::test]
    async fn clears_account_lock_on_success() {
        let limiter = LoginLimiter::in_memory();
        for _ in 0..MAX_ACCOUNT_ATTEMPTS {
            limiter.record_failure_account("EMP001").await.unwrap();
        }
        limiter.clear_account("EMP001").await.unwrap();
        assert!(!limiter.is_locked_account("EMP001").await.unwrap());
    }

    #[tokio::test]
    async fn locks_pin_change_account_after_max_failures() {
        let limiter = LoginLimiter::in_memory();
        for _ in 0..MAX_PIN_CHANGE_ACCOUNT_ATTEMPTS {
            limiter
                .record_pin_change_failure_account("EMP001")
                .await
                .unwrap();
        }
        assert!(limiter
            .is_locked_pin_change_account("EMP001")
            .await
            .unwrap());
    }
}
