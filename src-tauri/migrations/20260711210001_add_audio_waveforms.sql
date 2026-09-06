ALTER TABLE audio_chunks
    ADD COLUMN waveform_json TEXT NOT NULL DEFAULT '[]';
