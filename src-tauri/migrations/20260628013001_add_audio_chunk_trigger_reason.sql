ALTER TABLE audio_chunks
    ADD COLUMN trigger_reason TEXT NOT NULL DEFAULT 'static_fallback';
