-- Create OCR results table
CREATE TABLE IF NOT EXISTS ocr_results (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    frame_path TEXT,
    text TEXT NOT NULL,
    confidence REAL NOT NULL,
    bounding_box TEXT NOT NULL,
    language TEXT NOT NULL DEFAULT 'eng',
    processing_time_ms INTEGER,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

-- Create indexes for OCR results
CREATE INDEX IF NOT EXISTS idx_ocr_session ON ocr_results(session_id);
CREATE INDEX IF NOT EXISTS idx_ocr_timestamp ON ocr_results(timestamp);
CREATE INDEX IF NOT EXISTS idx_ocr_confidence ON ocr_results(confidence);

-- Create full-text search index for OCR text
CREATE VIRTUAL TABLE IF NOT EXISTS ocr_fts USING fts5(
    text,
    session_id UNINDEXED,
    timestamp UNINDEXED,
    content='ocr_results',
    content_rowid='rowid'
);

-- Trigger to keep FTS index in sync with ocr_results
CREATE TRIGGER IF NOT EXISTS ocr_fts_insert AFTER INSERT ON ocr_results BEGIN
    INSERT INTO ocr_fts(rowid, text, session_id, timestamp)
    VALUES (new.rowid, new.text, new.session_id, new.timestamp);
END;

CREATE TRIGGER IF NOT EXISTS ocr_fts_delete AFTER DELETE ON ocr_results BEGIN
    DELETE FROM ocr_fts WHERE rowid = old.rowid;
END;

CREATE TRIGGER IF NOT EXISTS ocr_fts_update AFTER UPDATE ON ocr_results BEGIN
    DELETE FROM ocr_fts WHERE rowid = old.rowid;
    INSERT INTO ocr_fts(rowid, text, session_id, timestamp)
    VALUES (new.rowid, new.text, new.session_id, new.timestamp);
END;
