CREATE TABLE IF NOT EXISTS mobile_clips (
    clip_id TEXT PRIMARY KEY,
    device_id TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at INTEGER NOT NULL,
    audio_path TEXT NOT NULL,
    bytes INTEGER NOT NULL DEFAULT 0,
    delivered_at INTEGER,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mobile_clips_device
    ON mobile_clips(device_id);
CREATE INDEX IF NOT EXISTS idx_mobile_clips_started
    ON mobile_clips(started_at);
