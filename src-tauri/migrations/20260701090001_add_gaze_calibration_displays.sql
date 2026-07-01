ALTER TABLE gaze_calibrations
    ADD COLUMN display_id INTEGER;

ALTER TABLE gaze_calibrations
    ADD COLUMN display_name TEXT;

ALTER TABLE gaze_calibrations
    ADD COLUMN display_x INTEGER NOT NULL DEFAULT 0;

ALTER TABLE gaze_calibrations
    ADD COLUMN display_y INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_gaze_calibrations_display_active
    ON gaze_calibrations(display_id, active, created_at DESC);
