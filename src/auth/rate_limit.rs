use std::collections::HashMap;
use std::time::{Duration, Instant};

pub fn retain_recent(entry: &mut Vec<Instant>, now: Instant, window: Duration) -> usize {
    entry.retain(|timestamp| now.duration_since(*timestamp) < window);
    entry.len()
}

pub fn prune_stale_keys(map: &mut HashMap<String, Vec<Instant>>, now: Instant, window: Duration) {
    map.retain(|_, entry| retain_recent(entry, now, window) > 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_stale_keys_removes_empty_entries() {
        let mut map = HashMap::new();
        map.insert(
            "ip:1.2.3.4".to_string(),
            vec![Instant::now() - Duration::from_secs(120)],
        );
        map.insert("ip:5.6.7.8".to_string(), vec![Instant::now()]);

        prune_stale_keys(&mut map, Instant::now(), Duration::from_secs(60));

        assert!(!map.contains_key("ip:1.2.3.4"));
        assert!(map.contains_key("ip:5.6.7.8"));
    }
}
