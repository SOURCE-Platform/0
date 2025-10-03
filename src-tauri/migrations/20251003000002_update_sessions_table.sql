-- Add new columns to sessions table for frame tracking
ALTER TABLE sessions ADD COLUMN frame_count INTEGER DEFAULT 0;
ALTER TABLE sessions ADD COLUMN total_size_bytes INTEGER DEFAULT 0;
ALTER TABLE sessions ADD COLUMN recording_path TEXT;
