CREATE TABLE IF NOT EXISTS mobile_devices (
    device_id TEXT PRIMARY KEY,
    device_name TEXT NOT NULL DEFAULT '',
    token_hash TEXT NOT NULL,
    last_seen_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mobile_devices_last_seen
    ON mobile_devices(last_seen_at);
