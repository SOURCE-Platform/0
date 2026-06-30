CREATE TABLE IF NOT EXISTS gaze_calibrations (
    calibration_id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT,
    created_at INTEGER NOT NULL,
    screen_width INTEGER NOT NULL,
    screen_height INTEGER NOT NULL,
    camera_id TEXT NOT NULL,
    model_name TEXT NOT NULL,
    model_version TEXT NOT NULL,
    calibration_points_json TEXT NOT NULL,
    validation_error_px REAL,
    validation_quality TEXT,
    head_pose_range_json TEXT,
    active INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_gaze_calibrations_active
    ON gaze_calibrations(active);

CREATE TABLE IF NOT EXISTS gaze_samples (
    gaze_sample_id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    calibration_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    screen_x REAL NOT NULL,
    screen_y REAL NOT NULL,
    confidence REAL NOT NULL,
    accuracy_radius_px REAL NOT NULL,
    head_pose_json TEXT,
    face_bbox_json TEXT,
    raw_features_ref TEXT,
    model_name TEXT NOT NULL,
    model_version TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_gaze_samples_session
    ON gaze_samples(session_id);
CREATE INDEX IF NOT EXISTS idx_gaze_samples_time
    ON gaze_samples(timestamp);

CREATE TABLE IF NOT EXISTS attention_snapshots (
    attention_snapshot_id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    gaze_sample_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    screen_x REAL NOT NULL,
    screen_y REAL NOT NULL,
    accuracy_radius_px REAL NOT NULL,
    confidence REAL NOT NULL,
    frontmost_app_name TEXT,
    window_title TEXT,
    likely_targets_json TEXT NOT NULL,
    resolver_version TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_attention_snapshots_session
    ON attention_snapshots(session_id);
CREATE INDEX IF NOT EXISTS idx_attention_snapshots_time
    ON attention_snapshots(timestamp);

CREATE TABLE IF NOT EXISTS attention_spans (
    attention_span_id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    label TEXT NOT NULL,
    first_seen_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    supporting_attention_snapshot_ids_json TEXT NOT NULL,
    avg_confidence REAL NOT NULL,
    max_confidence REAL NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_attention_spans_session
    ON attention_spans(session_id);
CREATE INDEX IF NOT EXISTS idx_attention_spans_time
    ON attention_spans(first_seen_at, last_seen_at);
