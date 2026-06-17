use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sqlx::PgPool;
use time::OffsetDateTime;

use crate::auth::rate_limit::{prune_stale_keys, retain_recent};
use crate::error::{AppError, AppResult};

const MAX_RETENTION: Duration = Duration::from_secs(15 * 60);

#[derive(Clone)]
pub enum RateLimitStore {
    Memory(Arc<Mutex<HashMap<String, Vec<Instant>>>>),
    Postgres(PgPool),
}

impl RateLimitStore {
    pub fn in_memory() -> Self {
        Self::Memory(Arc::new(Mutex::new(HashMap::new())))
    }

    pub fn postgres(pool: PgPool) -> Self {
        Self::Postgres(pool)
    }

    pub async fn count_recent(&self, key: &str, window: Duration) -> AppResult<usize> {
        match self {
            Self::Memory(store) => {
                let mut guard = store.lock().expect("rate limit store lock");
                let now = Instant::now();
                prune_stale_keys(&mut guard, now, window);
                let count = guard
                    .get_mut(key)
                    .map(|entry| retain_recent(entry, now, window))
                    .unwrap_or(0);
                Ok(count)
            }
            Self::Postgres(pool) => {
                let cutoff = OffsetDateTime::now_utc() - window;
                let count: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*)::bigint FROM rate_limit_events
                     WHERE bucket_key = $1 AND created_at >= $2",
                )
                .bind(key)
                .bind(cutoff)
                .fetch_one(pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
                Ok(count as usize)
            }
        }
    }

    pub async fn record(&self, key: &str, window: Duration) -> AppResult<()> {
        match self {
            Self::Memory(store) => {
                let mut guard = store.lock().expect("rate limit store lock");
                let now = Instant::now();
                prune_stale_keys(&mut guard, now, window);
                let entry = guard.entry(key.to_string()).or_default();
                retain_recent(entry, now, window);
                entry.push(now);
                Ok(())
            }
            Self::Postgres(pool) => {
                sqlx::query("INSERT INTO rate_limit_events (bucket_key) VALUES ($1)")
                    .bind(key)
                    .execute(pool)
                    .await
                    .map_err(|e| AppError::Internal(e.into()))?;
                Ok(())
            }
        }
    }

    pub async fn remove(&self, key: &str) -> AppResult<()> {
        match self {
            Self::Memory(store) => {
                let mut guard = store.lock().expect("rate limit store lock");
                guard.remove(key);
                Ok(())
            }
            Self::Postgres(pool) => {
                sqlx::query("DELETE FROM rate_limit_events WHERE bucket_key = $1")
                    .bind(key)
                    .execute(pool)
                    .await
                    .map_err(|e| AppError::Internal(e.into()))?;
                Ok(())
            }
        }
    }

    pub async fn cleanup_expired_postgres(pool: &PgPool) -> AppResult<()> {
        let cutoff = OffsetDateTime::now_utc() - MAX_RETENTION;
        sqlx::query("DELETE FROM rate_limit_events WHERE created_at < $1")
            .bind(cutoff)
            .execute(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_store_counts_and_records() {
        let store = RateLimitStore::in_memory();
        assert_eq!(
            store
                .count_recent("ip:1.2.3.4", Duration::from_secs(60))
                .await
                .unwrap(),
            0
        );
        store
            .record("ip:1.2.3.4", Duration::from_secs(60))
            .await
            .unwrap();
        assert_eq!(
            store
                .count_recent("ip:1.2.3.4", Duration::from_secs(60))
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn postgres_store_is_shared_across_handles() {
        let Some(pool) = test_pool().await else {
            eprintln!("skipping postgres rate limit test: DATABASE_URL not available");
            return;
        };

        let key = format!("test:shared:{}", uuid::Uuid::new_v4());
        let store_a = RateLimitStore::postgres(pool.clone());
        let store_b = RateLimitStore::postgres(pool.clone());

        store_a.record(&key, Duration::from_secs(60)).await.unwrap();
        assert_eq!(
            store_b
                .count_recent(&key, Duration::from_secs(60))
                .await
                .unwrap(),
            1
        );

        store_a.remove(&key).await.unwrap();
    }

    async fn test_pool() -> Option<PgPool> {
        dotenvy::dotenv().ok();
        let url = std::env::var("DATABASE_URL").ok()?;
        let pool = crate::db::connect(&url).await.ok()?;
        crate::db::migrate(&pool).await.ok()?;
        Some(pool)
    }
}
