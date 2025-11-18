-- Create pose_frames table for body and face tracking
CREATE TABLE IF NOT EXISTS pose_frames (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    frame_id TEXT,

    -- Body pose data (33 keypoints)
    body_keypoints_json TEXT,
    body_visibility_json TEXT,
    body_world_landmarks_json TEXT,
    pose_classification TEXT,

    -- Face mesh data (468 landmarks + 52 blendshapes)
    face_landmarks_json TEXT,
    face_blendshapes_json TEXT,
    face_transformation_matrix_json TEXT,

    -- Hand tracking data (21 keypoints per hand)
    left_hand_json TEXT,
    right_hand_json TEXT,

    processing_time_ms INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now') * 1000),

    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

-- Create indexes for pose_frames
CREATE INDEX IF NOT EXISTS idx_pose_session ON pose_frames(session_id);
CREATE INDEX IF NOT EXISTS idx_pose_timestamp ON pose_frames(timestamp);
CREATE INDEX IF NOT EXISTS idx_pose_classification ON pose_frames(pose_classification);

-- Create facial_expressions table for aggregated expression events
CREATE TABLE IF NOT EXISTS facial_expressions (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    expression_type TEXT NOT NULL,
    intensity REAL NOT NULL,
    duration_ms INTEGER,
    blendshapes_json TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now') * 1000),

    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

-- Create indexes for facial_expressions
CREATE INDEX IF NOT EXISTS idx_expressions_session ON facial_expressions(session_id);
CREATE INDEX IF NOT EXISTS idx_expressions_timestamp ON facial_expressions(timestamp);
CREATE INDEX IF NOT EXISTS idx_expressions_type ON facial_expressions(expression_type);

-- Add pose tracking flag to sessions table
ALTER TABLE sessions ADD COLUMN pose_tracking_enabled INTEGER DEFAULT 0;
