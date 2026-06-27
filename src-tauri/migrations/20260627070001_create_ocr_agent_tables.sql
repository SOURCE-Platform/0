CREATE TABLE IF NOT EXISTS ocr_scene_snapshots (
    scene_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    display_id INTEGER,
    frontmost_app_name TEXT,
    frontmost_bundle_id TEXT,
    window_title TEXT,
    trigger_reason TEXT NOT NULL,
    frame_path TEXT,
    frame_width INTEGER,
    frame_height INTEGER,
    full_text TEXT NOT NULL,
    avg_confidence REAL NOT NULL,
    block_count INTEGER NOT NULL,
    text_blocks_json TEXT NOT NULL,
    pii_entities_json TEXT NOT NULL,
    linked_context_json TEXT NOT NULL,
    raw_source_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ocr_scene_snapshots_session
    ON ocr_scene_snapshots(session_id);
CREATE INDEX IF NOT EXISTS idx_ocr_scene_snapshots_timestamp
    ON ocr_scene_snapshots(timestamp);
CREATE INDEX IF NOT EXISTS idx_ocr_scene_snapshots_app
    ON ocr_scene_snapshots(frontmost_app_name);

CREATE TABLE IF NOT EXISTS ocr_text_spans (
    text_span_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    canonical_text TEXT NOT NULL,
    normalized_text TEXT NOT NULL,
    first_seen_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    scene_ids_json TEXT NOT NULL,
    frontmost_app_name TEXT,
    frontmost_bundle_id TEXT,
    window_title TEXT,
    avg_confidence REAL NOT NULL,
    bbox_union_json TEXT NOT NULL,
    occurrence_count INTEGER NOT NULL,
    was_partial_match INTEGER NOT NULL,
    pii_entities_json TEXT NOT NULL,
    raw_source_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ocr_text_spans_session
    ON ocr_text_spans(session_id);
CREATE INDEX IF NOT EXISTS idx_ocr_text_spans_time
    ON ocr_text_spans(first_seen_at, last_seen_at);
CREATE INDEX IF NOT EXISTS idx_ocr_text_spans_app
    ON ocr_text_spans(frontmost_app_name);
CREATE INDEX IF NOT EXISTS idx_ocr_text_spans_normalized_text
    ON ocr_text_spans(normalized_text);

CREATE TABLE IF NOT EXISTS ocr_context_entities (
    entity_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    frontmost_app_name TEXT,
    frontmost_bundle_id TEXT,
    window_title TEXT,
    first_seen_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    scene_ids_json TEXT NOT NULL,
    text_span_ids_json TEXT NOT NULL,
    title_hint TEXT,
    summary_text TEXT NOT NULL,
    dominant_terms_json TEXT NOT NULL,
    pii_entity_counts_json TEXT NOT NULL,
    raw_source_json TEXT NOT NULL,
    confidence REAL NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ocr_context_entities_session
    ON ocr_context_entities(session_id);
CREATE INDEX IF NOT EXISTS idx_ocr_context_entities_time
    ON ocr_context_entities(first_seen_at, last_seen_at);
CREATE INDEX IF NOT EXISTS idx_ocr_context_entities_app
    ON ocr_context_entities(frontmost_app_name);
CREATE INDEX IF NOT EXISTS idx_ocr_context_entities_type
    ON ocr_context_entities(entity_type);
