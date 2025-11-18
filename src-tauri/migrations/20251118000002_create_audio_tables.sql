-- Create audio_recordings table
CREATE TABLE IF NOT EXISTS audio_recordings (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    start_timestamp INTEGER NOT NULL,
    end_timestamp INTEGER,
    sample_rate INTEGER NOT NULL,
    channels INTEGER NOT NULL,
    bit_depth INTEGER NOT NULL DEFAULT 16,
    file_path TEXT NOT NULL,
    file_size_bytes INTEGER,
    codec TEXT NOT NULL DEFAULT 'aac',
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now') * 1000),

    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

-- Create indexes for audio_recordings
CREATE INDEX IF NOT EXISTS idx_audio_recordings_session ON audio_recordings(session_id);
CREATE INDEX IF NOT EXISTS idx_audio_recordings_timestamp ON audio_recordings(start_timestamp);

-- Create audio_sources table for source separation metadata
CREATE TABLE IF NOT EXISTS audio_sources (
    id TEXT PRIMARY KEY,
    recording_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,

    has_user_speech INTEGER NOT NULL DEFAULT 0,
    has_system_audio INTEGER NOT NULL DEFAULT 0,
    system_audio_type TEXT,
    confidence REAL NOT NULL,
    speech_probability REAL DEFAULT 0.0,
    music_probability REAL DEFAULT 0.0,

    vocals_file_path TEXT,
    music_file_path TEXT,
    bass_file_path TEXT,
    other_file_path TEXT,

    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now') * 1000),

    FOREIGN KEY (recording_id) REFERENCES audio_recordings(id) ON DELETE CASCADE
);

-- Create indexes for audio_sources
CREATE INDEX IF NOT EXISTS idx_audio_sources_recording ON audio_sources(recording_id);
CREATE INDEX IF NOT EXISTS idx_audio_sources_timestamp ON audio_sources(timestamp);
CREATE INDEX IF NOT EXISTS idx_audio_sources_type ON audio_sources(system_audio_type);

-- Create transcripts table for speech transcription
CREATE TABLE IF NOT EXISTS transcripts (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    recording_id TEXT NOT NULL,
    start_timestamp INTEGER NOT NULL,
    end_timestamp INTEGER NOT NULL,
    text TEXT NOT NULL,
    language TEXT NOT NULL DEFAULT 'en',
    confidence REAL NOT NULL,
    speaker_id TEXT,
    words_json TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now') * 1000),

    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    FOREIGN KEY (recording_id) REFERENCES audio_recordings(id) ON DELETE CASCADE
);

-- Create indexes for transcripts
CREATE INDEX IF NOT EXISTS idx_transcripts_session ON transcripts(session_id);
CREATE INDEX IF NOT EXISTS idx_transcripts_recording ON transcripts(recording_id);
CREATE INDEX IF NOT EXISTS idx_transcripts_timestamp ON transcripts(start_timestamp);
CREATE INDEX IF NOT EXISTS idx_transcripts_speaker ON transcripts(speaker_id);

-- Create full-text search index for transcripts
CREATE VIRTUAL TABLE IF NOT EXISTS transcripts_fts USING fts5(
    text,
    session_id UNINDEXED,
    speaker_id UNINDEXED,
    start_timestamp UNINDEXED,
    content='transcripts',
    content_rowid='rowid'
);

-- Triggers to keep FTS index in sync with transcripts
CREATE TRIGGER IF NOT EXISTS transcripts_fts_insert AFTER INSERT ON transcripts BEGIN
    INSERT INTO transcripts_fts(rowid, text, session_id, speaker_id, start_timestamp)
    VALUES (new.rowid, new.text, new.session_id, new.speaker_id, new.start_timestamp);
END;

CREATE TRIGGER IF NOT EXISTS transcripts_fts_delete AFTER DELETE ON transcripts BEGIN
    DELETE FROM transcripts_fts WHERE rowid = old.rowid;
END;

CREATE TRIGGER IF NOT EXISTS transcripts_fts_update AFTER UPDATE ON transcripts BEGIN
    DELETE FROM transcripts_fts WHERE rowid = old.rowid;
    INSERT INTO transcripts_fts(rowid, text, session_id, speaker_id, start_timestamp)
    VALUES (new.rowid, new.text, new.session_id, new.speaker_id, new.start_timestamp);
END;

-- Create speaker_segments table for speaker diarization
CREATE TABLE IF NOT EXISTS speaker_segments (
    id TEXT PRIMARY KEY,
    recording_id TEXT NOT NULL,
    speaker_id TEXT NOT NULL,
    start_timestamp INTEGER NOT NULL,
    end_timestamp INTEGER NOT NULL,
    confidence REAL NOT NULL,
    embedding_json TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now') * 1000),

    FOREIGN KEY (recording_id) REFERENCES audio_recordings(id) ON DELETE CASCADE
);

-- Create indexes for speaker_segments
CREATE INDEX IF NOT EXISTS idx_speakers_recording ON speaker_segments(recording_id);
CREATE INDEX IF NOT EXISTS idx_speakers_id ON speaker_segments(speaker_id);
CREATE INDEX IF NOT EXISTS idx_speakers_timestamp ON speaker_segments(start_timestamp);

-- Create emotion_detections table
CREATE TABLE IF NOT EXISTS emotion_detections (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    recording_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    speaker_id TEXT,
    emotion TEXT NOT NULL,
    confidence REAL NOT NULL,
    valence REAL NOT NULL,
    arousal REAL NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now') * 1000),

    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    FOREIGN KEY (recording_id) REFERENCES audio_recordings(id) ON DELETE CASCADE
);

-- Create indexes for emotion_detections
CREATE INDEX IF NOT EXISTS idx_emotions_session ON emotion_detections(session_id);
CREATE INDEX IF NOT EXISTS idx_emotions_recording ON emotion_detections(recording_id);
CREATE INDEX IF NOT EXISTS idx_emotions_speaker ON emotion_detections(speaker_id);
CREATE INDEX IF NOT EXISTS idx_emotions_type ON emotion_detections(emotion);
CREATE INDEX IF NOT EXISTS idx_emotions_timestamp ON emotion_detections(timestamp);

-- Add audio recording flag to sessions table
ALTER TABLE sessions ADD COLUMN audio_recording_enabled INTEGER DEFAULT 0;
