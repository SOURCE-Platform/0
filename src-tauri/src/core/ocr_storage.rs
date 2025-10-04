// OCR storage and database operations

use crate::core::database::Database;
use crate::models::ocr::{BoundingBox, OcrResult, TextBlock};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

// ==============================================================================
// Errors
// ==============================================================================

#[derive(Debug, Error)]
pub enum OcrStorageError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Invalid data: {0}")]
    InvalidData(String),
}

type Result<T> = std::result::Result<T, OcrStorageError>;

// ==============================================================================
// Processed OCR Result (for storage)
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessedOcrResult {
    pub session_id: Uuid,
    pub timestamp: i64,
    pub frame_path: Option<PathBuf>,
    pub ocr_result: OcrResult,
}

// ==============================================================================
// OCR Storage
// ==============================================================================

pub struct OcrStorage {
    db: Arc<Database>,
}

impl OcrStorage {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Save OCR result to database
    pub async fn save_ocr_result(&self, result: ProcessedOcrResult) -> Result<()> {
        let pool = self.db.pool();
        let created_at = chrono::Utc::now().timestamp();

        // Save each text block as a separate row
        for text_block in &result.ocr_result.text_blocks {
            let id = Uuid::new_v4().to_string();
            let session_id = result.session_id.to_string();
            let frame_path = result
                .frame_path
                .as_ref()
                .and_then(|p| p.to_str())
                .unwrap_or("");
            let bounding_box = serde_json::to_string(&text_block.bounding_box)?;

            sqlx::query(
                r#"
                INSERT INTO ocr_results (
                    id, session_id, timestamp, frame_path, text,
                    confidence, bounding_box, language, processing_time_ms, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(id)
            .bind(session_id)
            .bind(result.timestamp)
            .bind(frame_path)
            .bind(&text_block.text)
            .bind(text_block.confidence)
            .bind(bounding_box)
            .bind(&text_block.language)
            .bind(result.ocr_result.processing_time_ms as i64)
            .bind(created_at)
            .execute(pool)
            .await?;
        }

        Ok(())
    }

    /// Get OCR results for a session
    pub async fn get_session_ocr_results(
        &self,
        session_id: Uuid,
        limit: Option<u32>,
    ) -> Result<Vec<StoredOcrResult>> {
        let pool = self.db.pool();
        let session_id_str = session_id.to_string();

        let limit_clause = limit.map(|l| format!("LIMIT {}", l)).unwrap_or_default();

        let query = format!(
            r#"
            SELECT id, session_id, timestamp, frame_path, text,
                   confidence, bounding_box, language, processing_time_ms
            FROM ocr_results
            WHERE session_id = ?
            ORDER BY timestamp DESC
            {}
            "#,
            limit_clause
        );

        let rows = sqlx::query_as::<_, OcrResultRow>(&query)
            .bind(session_id_str)
            .fetch_all(pool)
            .await?;

        rows.into_iter().map(|row| row.try_into()).collect()
    }

    /// Search OCR results by text using full-text search
    pub async fn search_text(
        &self,
        query: &str,
        session_id: Option<Uuid>,
        limit: u32,
    ) -> Result<Vec<SearchResult>> {
        let pool = self.db.pool();

        let (search_query, params): (String, Vec<String>) = if let Some(sid) = session_id {
            (
                r#"
                SELECT o.id, o.session_id, o.timestamp, o.text, o.confidence,
                       o.frame_path, o.bounding_box
                FROM ocr_fts f
                JOIN ocr_results o ON f.rowid = o.rowid
                WHERE f.text MATCH ? AND o.session_id = ?
                ORDER BY rank
                LIMIT ?
                "#
                .to_string(),
                vec![query.to_string(), sid.to_string(), limit.to_string()],
            )
        } else {
            (
                r#"
                SELECT o.id, o.session_id, o.timestamp, o.text, o.confidence,
                       o.frame_path, o.bounding_box
                FROM ocr_fts f
                JOIN ocr_results o ON f.rowid = o.rowid
                WHERE f.text MATCH ?
                ORDER BY rank
                LIMIT ?
                "#
                .to_string(),
                vec![query.to_string(), limit.to_string()],
            )
        };

        let mut query_builder = sqlx::query_as::<_, SearchResultRow>(&search_query);
        for param in params {
            query_builder = query_builder.bind(param);
        }

        let rows = query_builder.fetch_all(pool).await?;

        rows.into_iter().map(|row| row.try_into()).collect()
    }

    /// Get OCR coverage statistics for a session
    pub async fn get_ocr_stats(&self, session_id: Uuid) -> Result<OcrStats> {
        let pool = self.db.pool();
        let session_id_str = session_id.to_string();

        let stats_row = sqlx::query_as::<_, OcrStatsRow>(
            r#"
            SELECT
                COUNT(DISTINCT timestamp) as frames_processed,
                COUNT(*) as text_blocks_extracted,
                AVG(processing_time_ms) as avg_processing_time_ms,
                AVG(confidence) as avg_confidence,
                SUM(LENGTH(text)) as total_text_length
            FROM ocr_results
            WHERE session_id = ?
            "#,
        )
        .bind(session_id_str)
        .fetch_one(pool)
        .await?;

        Ok(OcrStats {
            frames_processed: stats_row.frames_processed as u64,
            text_blocks_extracted: stats_row.text_blocks_extracted as u64,
            average_processing_time_ms: stats_row.avg_processing_time_ms,
            average_confidence: stats_row.avg_confidence,
            total_text_length: stats_row.total_text_length as u64,
        })
    }

    /// Delete OCR results older than specified days
    pub async fn cleanup_old_results(&self, retention_days: u32) -> Result<u64> {
        let pool = self.db.pool();
        let cutoff_timestamp = chrono::Utc::now()
            .timestamp()
            - (retention_days as i64 * 24 * 60 * 60);

        let result = sqlx::query("DELETE FROM ocr_results WHERE created_at < ?")
            .bind(cutoff_timestamp)
            .execute(pool)
            .await?;

        Ok(result.rows_affected())
    }
}

// ==============================================================================
// Database Row Types
// ==============================================================================

#[derive(Debug, sqlx::FromRow)]
struct OcrResultRow {
    id: String,
    session_id: String,
    timestamp: i64,
    frame_path: Option<String>,
    text: String,
    confidence: f64,
    bounding_box: String,
    language: String,
    processing_time_ms: Option<i64>,
}

impl TryFrom<OcrResultRow> for StoredOcrResult {
    type Error = OcrStorageError;

    fn try_from(row: OcrResultRow) -> Result<Self> {
        let bounding_box: BoundingBox = serde_json::from_str(&row.bounding_box)?;

        Ok(StoredOcrResult {
            id: row.id,
            session_id: Uuid::parse_str(&row.session_id)
                .map_err(|e| OcrStorageError::InvalidData(e.to_string()))?,
            timestamp: row.timestamp,
            frame_path: row.frame_path.map(PathBuf::from),
            text: row.text,
            confidence: row.confidence as f32,
            bounding_box,
            language: row.language,
            processing_time_ms: row.processing_time_ms.map(|t| t as u64),
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
struct SearchResultRow {
    id: String,
    session_id: String,
    timestamp: i64,
    text: String,
    confidence: f64,
    frame_path: Option<String>,
    bounding_box: String,
}

impl TryFrom<SearchResultRow> for SearchResult {
    type Error = OcrStorageError;

    fn try_from(row: SearchResultRow) -> Result<Self> {
        let bounding_box: BoundingBox = serde_json::from_str(&row.bounding_box)?;

        Ok(SearchResult {
            id: row.id,
            session_id: Uuid::parse_str(&row.session_id)
                .map_err(|e| OcrStorageError::InvalidData(e.to_string()))?,
            timestamp: row.timestamp,
            text: row.text,
            confidence: row.confidence as f32,
            frame_path: row.frame_path.map(PathBuf::from),
            bounding_box,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
struct OcrStatsRow {
    frames_processed: i64,
    text_blocks_extracted: i64,
    avg_processing_time_ms: f64,
    avg_confidence: f64,
    total_text_length: i64,
}

// ==============================================================================
// Public Types
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredOcrResult {
    pub id: String,
    pub session_id: Uuid,
    pub timestamp: i64,
    pub frame_path: Option<PathBuf>,
    pub text: String,
    pub confidence: f32,
    pub bounding_box: BoundingBox,
    pub language: String,
    pub processing_time_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub session_id: Uuid,
    pub timestamp: i64,
    pub text: String,
    pub confidence: f32,
    pub frame_path: Option<PathBuf>,
    pub bounding_box: BoundingBox,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrStats {
    pub frames_processed: u64,
    pub text_blocks_extracted: u64,
    pub average_processing_time_ms: f64,
    pub average_confidence: f64,
    pub total_text_length: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_processed_ocr_result_serialization() {
        let result = ProcessedOcrResult {
            session_id: Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            frame_path: Some(PathBuf::from("/path/to/frame.png")),
            ocr_result: OcrResult::new(0, vec![], 100),
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: ProcessedOcrResult = serde_json::from_str(&json).unwrap();

        assert_eq!(result.session_id, deserialized.session_id);
    }
}
