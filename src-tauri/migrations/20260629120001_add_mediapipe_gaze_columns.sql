ALTER TABLE gaze_samples
    ADD COLUMN gaze_vector_json TEXT;

ALTER TABLE gaze_samples
    ADD COLUMN projected_point_json TEXT;

ALTER TABLE gaze_samples
    ADD COLUMN landmark_payload_json TEXT;
