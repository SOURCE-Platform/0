-- Add dynamic recording fields to sessions table
ALTER TABLE sessions ADD COLUMN base_layer_path TEXT;
ALTER TABLE sessions ADD COLUMN segment_count INTEGER DEFAULT 0;
ALTER TABLE sessions ADD COLUMN total_motion_percentage REAL DEFAULT 0.0;

-- Create screen_recordings table for tracking display-specific recordings
CREATE TABLE IF NOT EXISTS screen_recordings (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    display_id INTEGER NOT NULL,
    start_timestamp INTEGER NOT NULL,
    end_timestamp INTEGER,
    base_layer_path TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now') * 1000),
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

-- Index for session-based queries
CREATE INDEX IF NOT EXISTS idx_screen_recordings_session ON screen_recordings(session_id);

-- Index for display-based queries
CREATE INDEX IF NOT EXISTS idx_screen_recordings_display ON screen_recordings(display_id);
