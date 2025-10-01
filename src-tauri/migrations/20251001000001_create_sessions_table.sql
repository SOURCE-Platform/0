-- Create sessions table
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY NOT NULL,
    start_timestamp INTEGER NOT NULL,
    end_timestamp INTEGER,
    device_id TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

-- Create index on start_timestamp for faster queries
CREATE INDEX IF NOT EXISTS idx_sessions_start_timestamp ON sessions(start_timestamp);

-- Create index on device_id for device-specific queries
CREATE INDEX IF NOT EXISTS idx_sessions_device_id ON sessions(device_id);
