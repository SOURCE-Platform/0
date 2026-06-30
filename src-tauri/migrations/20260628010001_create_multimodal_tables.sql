CREATE TABLE IF NOT EXISTS video_frame_samples (
    frame_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    source_id TEXT NOT NULL,
    width INTEGER NOT NULL,
    height INTEGER NOT NULL,
    frame_path TEXT,
    retained_as_evidence INTEGER NOT NULL DEFAULT 0,
    sampling_reason TEXT NOT NULL,
    motion_score REAL NOT NULL DEFAULT 0,
    scene_delta REAL NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_video_frame_samples_session
    ON video_frame_samples(session_id);
CREATE INDEX IF NOT EXISTS idx_video_frame_samples_timestamp
    ON video_frame_samples(timestamp);
CREATE INDEX IF NOT EXISTS idx_video_frame_samples_source
    ON video_frame_samples(source_id);

CREATE TABLE IF NOT EXISTS vision_detections (
    detection_id TEXT PRIMARY KEY,
    frame_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    detector_type TEXT NOT NULL,
    model_name TEXT NOT NULL,
    model_version TEXT NOT NULL,
    raw_json TEXT NOT NULL,
    confidence REAL NOT NULL,
    processing_time_ms INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_vision_detections_frame
    ON vision_detections(frame_id);
CREATE INDEX IF NOT EXISTS idx_vision_detections_session
    ON vision_detections(session_id);
CREATE INDEX IF NOT EXISTS idx_vision_detections_timestamp
    ON vision_detections(timestamp);
CREATE INDEX IF NOT EXISTS idx_vision_detections_type
    ON vision_detections(detector_type);

CREATE TABLE IF NOT EXISTS visual_scene_snapshots (
    visual_scene_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    source_id TEXT NOT NULL,
    trigger_reason TEXT NOT NULL,
    frame_id TEXT,
    person_count INTEGER NOT NULL DEFAULT 0,
    presence_label TEXT NOT NULL,
    presence_confidence REAL NOT NULL,
    posture_label TEXT NOT NULL,
    posture_confidence REAL NOT NULL,
    motion_label TEXT NOT NULL,
    motion_confidence REAL NOT NULL,
    object_labels_json TEXT NOT NULL,
    object_boxes_json TEXT NOT NULL,
    fused_state_json TEXT NOT NULL,
    avg_confidence REAL NOT NULL,
    processing_time_ms INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_visual_scene_snapshots_session
    ON visual_scene_snapshots(session_id);
CREATE INDEX IF NOT EXISTS idx_visual_scene_snapshots_timestamp
    ON visual_scene_snapshots(timestamp);
CREATE INDEX IF NOT EXISTS idx_visual_scene_snapshots_source
    ON visual_scene_snapshots(source_id);

CREATE TABLE IF NOT EXISTS visual_state_spans (
    visual_state_span_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    state_type TEXT NOT NULL,
    label TEXT NOT NULL,
    first_seen_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    scene_ids_json TEXT NOT NULL,
    avg_confidence REAL NOT NULL,
    min_confidence REAL NOT NULL,
    max_confidence REAL NOT NULL,
    transition_in TEXT,
    transition_out TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_visual_state_spans_session
    ON visual_state_spans(session_id);
CREATE INDEX IF NOT EXISTS idx_visual_state_spans_time
    ON visual_state_spans(first_seen_at, last_seen_at);
CREATE INDEX IF NOT EXISTS idx_visual_state_spans_type
    ON visual_state_spans(state_type, label);

CREATE TABLE IF NOT EXISTS audio_chunks (
    audio_chunk_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    start_timestamp INTEGER NOT NULL,
    end_timestamp INTEGER NOT NULL,
    audio_path TEXT,
    retained_as_evidence INTEGER NOT NULL DEFAULT 0,
    vad_score REAL NOT NULL,
    speech_detected INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audio_chunks_session
    ON audio_chunks(session_id);
CREATE INDEX IF NOT EXISTS idx_audio_chunks_time
    ON audio_chunks(start_timestamp, end_timestamp);
CREATE INDEX IF NOT EXISTS idx_audio_chunks_source
    ON audio_chunks(source_id);

CREATE TABLE IF NOT EXISTS asr_segments (
    asr_segment_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    start_timestamp INTEGER NOT NULL,
    end_timestamp INTEGER NOT NULL,
    language TEXT,
    transcript TEXT NOT NULL,
    confidence REAL,
    model_name TEXT NOT NULL,
    model_version TEXT NOT NULL,
    audio_chunk_ids_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_asr_segments_session
    ON asr_segments(session_id);
CREATE INDEX IF NOT EXISTS idx_asr_segments_time
    ON asr_segments(start_timestamp, end_timestamp);

CREATE TABLE IF NOT EXISTS audio_state_spans (
    audio_state_span_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    state_type TEXT NOT NULL,
    label TEXT NOT NULL,
    first_seen_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    supporting_audio_chunk_ids_json TEXT NOT NULL,
    supporting_asr_segment_ids_json TEXT NOT NULL,
    avg_confidence REAL NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audio_state_spans_session
    ON audio_state_spans(session_id);
CREATE INDEX IF NOT EXISTS idx_audio_state_spans_time
    ON audio_state_spans(first_seen_at, last_seen_at);
CREATE INDEX IF NOT EXISTS idx_audio_state_spans_type
    ON audio_state_spans(state_type, label);
