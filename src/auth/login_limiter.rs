use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MAX_ATTEMPTS: usize = 5;
const WINDOW: Duration = Duration::from_secs(15 * 60);

pub struct LoginLimiter {
    attempts: Mutex<HashMap<String, Vec<Instant>>>,
}

impl LoginLimiter {
    pub fn new() -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
        }
    }

    pub fn is_locked(&self, key: &str) -> bool {
        let mut guard = self.attempts.lock().expect("login limiter lock");
        let now = Instant::now();
        let entry = guard.entry(key.to_string()).or_default();
        entry.retain(|t| now.duration_since(*t) < WINDOW);
        entry.len() >= MAX_ATTEMPTS
    }

    pub fn record_failure(&self, key: &str) {
        let mut guard = self.attempts.lock().expect("login limiter lock");
        let now = Instant::now();
        let entry = guard.entry(key.to_string()).or_default();
        entry.retain(|t| now.duration_since(*t) < WINDOW);
        entry.push(now);
    }

    pub fn clear(&self, key: &str) {
        let mut guard = self.attempts.lock().expect("login limiter lock");
        guard.remove(key);
    }
}

impl Default for LoginLimiter {
    fn default() -> Self {
        Self::new()
    }
}