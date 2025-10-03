-- Create video_segments table for tracking encoded video files
CREATE TABLE video_segments (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    start_timestamp INTEGER NOT NULL,
    end_timestamp INTEGER NOT NULL,
    file_path TEXT NOT NULL,
    frame_count INTEGER NOT NULL,
    file_size_bytes INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now') * 1000),
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

-- Index for efficient session-based queries
CREATE INDEX idx_segments_session ON video_segments(session_id);

-- Index for timestamp-based queries
CREATE INDEX idx_segments_timestamp ON video_segments(start_timestamp, end_timestamp);
