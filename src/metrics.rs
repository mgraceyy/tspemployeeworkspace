use std::sync::atomic::{AtomicU64, Ordering};

/// Upper bounds for Prometheus histogram buckets (seconds). The final `+Inf` bucket is implicit.
const DURATION_BUCKET_BOUNDS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

#[derive(Default)]
pub struct AppMetrics {
    http_requests_total: AtomicU64,
    http_errors_total: AtomicU64,
    http_duration_buckets: [AtomicU64; DURATION_BUCKET_BOUNDS.len() + 1],
    http_duration_sum_micros: AtomicU64,
    http_duration_count: AtomicU64,
}

impl AppMetrics {
    pub fn record_request(&self) {
        self.http_requests_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_error(&self) {
        self.http_errors_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_request_duration(&self, duration_secs: f64) {
        self.http_duration_count.fetch_add(1, Ordering::Relaxed);
        let micros = (duration_secs * 1_000_000.0).round() as u64;
        self.http_duration_sum_micros
            .fetch_add(micros, Ordering::Relaxed);

        for (i, &bound) in DURATION_BUCKET_BOUNDS.iter().enumerate() {
            if duration_secs <= bound {
                self.http_duration_buckets[i].fetch_add(1, Ordering::Relaxed);
            }
        }
        self.http_duration_buckets[DURATION_BUCKET_BOUNDS.len()].fetch_add(1, Ordering::Relaxed);
    }

    pub fn render_prometheus(&self) -> String {
        let requests = self.http_requests_total.load(Ordering::Relaxed);
        let errors = self.http_errors_total.load(Ordering::Relaxed);
        let count = self.http_duration_count.load(Ordering::Relaxed);
        let sum_secs = self.http_duration_sum_micros.load(Ordering::Relaxed) as f64 / 1_000_000.0;

        let mut out = String::from(
            "# HELP dtr_http_requests_total Total HTTP requests handled by the app\n\
             # TYPE dtr_http_requests_total counter\n",
        );
        out.push_str(&format!("dtr_http_requests_total {requests}\n"));
        out.push_str(
            "# HELP dtr_http_errors_total Total HTTP 5xx responses\n\
             # TYPE dtr_http_errors_total counter\n",
        );
        out.push_str(&format!("dtr_http_errors_total {errors}\n"));
        out.push_str(
            "# HELP dtr_http_request_duration_seconds HTTP request latency\n\
             # TYPE dtr_http_request_duration_seconds histogram\n",
        );

        for (i, &bound) in DURATION_BUCKET_BOUNDS.iter().enumerate() {
            let bucket_count = self.http_duration_buckets[i].load(Ordering::Relaxed);
            out.push_str(&format!(
                "dtr_http_request_duration_seconds_bucket{{le=\"{bound}\"}} {bucket_count}\n"
            ));
        }
        let inf_count =
            self.http_duration_buckets[DURATION_BUCKET_BOUNDS.len()].load(Ordering::Relaxed);
        out.push_str(&format!(
            "dtr_http_request_duration_seconds_bucket{{le=\"+Inf\"}} {inf_count}\n"
        ));
        out.push_str(&format!(
            "dtr_http_request_duration_seconds_sum {sum_secs}\n\
             dtr_http_request_duration_seconds_count {count}\n"
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_increment() {
        let metrics = AppMetrics::default();
        metrics.record_request();
        metrics.record_request();
        metrics.record_error();
        let rendered = metrics.render_prometheus();
        assert!(rendered.contains("dtr_http_requests_total 2"));
        assert!(rendered.contains("dtr_http_errors_total 1"));
    }

    #[test]
    fn duration_histogram_renders() {
        let metrics = AppMetrics::default();
        metrics.record_request_duration(0.002);
        metrics.record_request_duration(0.05);
        let rendered = metrics.render_prometheus();
        assert!(rendered.contains("dtr_http_request_duration_seconds_bucket"));
        assert!(rendered.contains("dtr_http_request_duration_seconds_count 2"));
        assert!(rendered.contains("dtr_http_request_duration_seconds_sum"));
    }
}
