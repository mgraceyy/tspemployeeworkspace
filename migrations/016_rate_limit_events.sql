CREATE TABLE rate_limit_events (
    id BIGSERIAL PRIMARY KEY,
    bucket_key TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_rate_limit_events_key_created
    ON rate_limit_events (bucket_key, created_at DESC);