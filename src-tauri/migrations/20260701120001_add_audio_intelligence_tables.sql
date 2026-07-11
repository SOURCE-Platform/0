CREATE TABLE IF NOT EXISTS speech_emotion_segments (
    speech_emotion_segment_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    audio_chunk_id TEXT NOT NULL,
    asr_segment_id TEXT,
    start_timestamp INTEGER NOT NULL,
    end_timestamp INTEGER NOT NULL,
    trigger_reason TEXT NOT NULL,
    emotion_label TEXT NOT NULL,
    canonical_label TEXT NOT NULL,
    confidence REAL NOT NULL,
    model_name TEXT NOT NULL,
    model_version TEXT NOT NULL,
    raw_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_speech_emotion_segments_session
    ON speech_emotion_segments(session_id);
CREATE INDEX IF NOT EXISTS idx_speech_emotion_segments_time
    ON speech_emotion_segments(start_timestamp, end_timestamp);
CREATE INDEX IF NOT EXISTS idx_speech_emotion_segments_source
    ON speech_emotion_segments(source_id);

CREATE TABLE IF NOT EXISTS sound_event_detections (
    sound_event_detection_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    audio_chunk_id TEXT NOT NULL,
    start_timestamp INTEGER NOT NULL,
    end_timestamp INTEGER NOT NULL,
    trigger_reason TEXT NOT NULL,
    event_label TEXT NOT NULL,
    canonical_label TEXT NOT NULL,
    confidence REAL NOT NULL,
    model_name TEXT NOT NULL,
    model_version TEXT NOT NULL,
    raw_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sound_event_detections_session
    ON sound_event_detections(session_id);
CREATE INDEX IF NOT EXISTS idx_sound_event_detections_time
    ON sound_event_detections(start_timestamp, end_timestamp);
CREATE INDEX IF NOT EXISTS idx_sound_event_detections_source
    ON sound_event_detections(source_id);
CREATE INDEX IF NOT EXISTS idx_sound_event_detections_label
    ON sound_event_detections(canonical_label, confidence);

CREATE TABLE IF NOT EXISTS sound_event_spans (
    sound_event_span_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    canonical_label TEXT NOT NULL,
    first_seen_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    supporting_detection_ids_json TEXT NOT NULL,
    supporting_audio_chunk_ids_json TEXT NOT NULL,
    avg_confidence REAL NOT NULL,
    max_confidence REAL NOT NULL,
    model_name TEXT NOT NULL,
    model_version TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sound_event_spans_session
    ON sound_event_spans(session_id);
CREATE INDEX IF NOT EXISTS idx_sound_event_spans_time
    ON sound_event_spans(first_seen_at, last_seen_at);
CREATE INDEX IF NOT EXISTS idx_sound_event_spans_label
    ON sound_event_spans(canonical_label, avg_confidence);
