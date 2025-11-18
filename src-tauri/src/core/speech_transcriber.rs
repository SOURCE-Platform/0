use crate::core::database::Database;
use crate::models::audio::{
    TranscriptSegment, WordTimestamp, AudioError, AudioResult,
    WhisperModelSize,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// ==============================================================================
// Database Models
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TranscriptRecord {
    pub id: String,
    pub session_id: String,
    pub recording_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub text: String,
    pub language: String,
    pub confidence: f64,
    pub speaker_id: Option<String>,
    pub words_json: Option<String>,
    pub created_at: i64,
}

// ==============================================================================
// Speech Transcriber
// ==============================================================================

pub struct SpeechTranscriber {
    db: Arc<Database>,
    model_size: Arc<RwLock<WhisperModelSize>>,
    language: Arc<RwLock<Option<String>>>,
}

impl SpeechTranscriber {
    pub async fn new(
        db: Arc<Database>,
        model_size: WhisperModelSize,
    ) -> AudioResult<Self> {
        Ok(Self {
            db,
            model_size: Arc::new(RwLock::new(model_size)),
            language: Arc::new(RwLock::new(None)),
        })
    }

    /// Transcribe an audio file
    pub async fn transcribe_audio(
        &self,
        session_id: &str,
        recording_id: &str,
        audio_file_path: &str,
    ) -> AudioResult<Vec<TranscriptSegment>> {
        // TODO: Implement actual Whisper transcription
        // This is a placeholder implementation

        // In a real implementation:
        // 1. Load Whisper model based on model_size
        // 2. Process audio file in chunks
        // 3. Run inference to get transcription with word timestamps
        // 4. Return segments with speaker attribution (if diarization is available)

        println!("Transcribing audio file: {}", audio_file_path);

        // Placeholder: return empty segments
        Ok(vec![])
    }

    /// Store transcript segment in database
    pub async fn store_transcript(&self, segment: &TranscriptSegment) -> AudioResult<()> {
        let pool = self.db.pool();
        let created_at = chrono::Utc::now().timestamp_millis();

        let words_json = serde_json::to_string(&segment.words)
            .map_err(|e| AudioError::TranscriptionFailed(e.to_string()))?;

        sqlx::query(
            "INSERT INTO transcripts (
                id, session_id, recording_id, start_timestamp, end_timestamp,
                text, language, confidence, speaker_id, words_json, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&segment.id)
        .bind(&segment.session_id)
        .bind(&segment.recording_id)
        .bind(segment.start_timestamp)
        .bind(segment.end_timestamp)
        .bind(&segment.text)
        .bind(&segment.language)
        .bind(segment.confidence as f64)
        .bind(&segment.speaker_id)
        .bind(words_json)
        .bind(created_at)
        .execute(pool)
        .await
        .map_err(|e| AudioError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    /// Get transcripts for a time range
    pub async fn get_transcripts(
        &self,
        session_id: &str,
        start: i64,
        end: i64,
        speaker_id: Option<String>,
    ) -> AudioResult<Vec<TranscriptSegment>> {
        let pool = self.db.pool();

        let records: Vec<TranscriptRecord> = if let Some(speaker) = speaker_id {
            sqlx::query_as(
                "SELECT * FROM transcripts
                 WHERE session_id = ? AND speaker_id = ? AND start_timestamp >= ? AND end_timestamp <= ?
                 ORDER BY start_timestamp ASC"
            )
            .bind(session_id)
            .bind(speaker)
            .bind(start)
            .bind(end)
            .fetch_all(pool)
            .await
        } else {
            sqlx::query_as(
                "SELECT * FROM transcripts
                 WHERE session_id = ? AND start_timestamp >= ? AND end_timestamp <= ?
                 ORDER BY start_timestamp ASC"
            )
            .bind(session_id)
            .bind(start)
            .bind(end)
            .fetch_all(pool)
            .await
        }
        .map_err(|e| AudioError::DatabaseError(e.to_string()))?;

        let segments = records.into_iter().map(|r| {
            let words = r.words_json
                .and_then(|json| serde_json::from_str::<Vec<WordTimestamp>>(&json).ok())
                .unwrap_or_default();

            TranscriptSegment {
                id: r.id,
                session_id: r.session_id,
                recording_id: r.recording_id,
                start_timestamp: r.start_timestamp,
                end_timestamp: r.end_timestamp,
                text: r.text,
                language: r.language,
                confidence: r.confidence as f32,
                speaker_id: r.speaker_id,
                words,
            }
        }).collect();

        Ok(segments)
    }

    /// Search transcripts using full-text search
    pub async fn search_transcripts(
        &self,
        query: &str,
        session_id: Option<String>,
        limit: i64,
        offset: i64,
    ) -> AudioResult<Vec<TranscriptSegment>> {
        let pool = self.db.pool();

        let records: Vec<TranscriptRecord> = if let Some(sid) = session_id {
            sqlx::query_as(
                "SELECT t.* FROM transcripts t
                 JOIN transcripts_fts fts ON t.rowid = fts.rowid
                 WHERE fts.text MATCH ? AND t.session_id = ?
                 ORDER BY rank
                 LIMIT ? OFFSET ?"
            )
            .bind(query)
            .bind(sid)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        } else {
            sqlx::query_as(
                "SELECT t.* FROM transcripts t
                 JOIN transcripts_fts fts ON t.rowid = fts.rowid
                 WHERE fts.text MATCH ?
                 ORDER BY rank
                 LIMIT ? OFFSET ?"
            )
            .bind(query)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
        .map_err(|e| AudioError::DatabaseError(e.to_string()))?;

        let segments = records.into_iter().map(|r| {
            let words = r.words_json
                .and_then(|json| serde_json::from_str::<Vec<WordTimestamp>>(&json).ok())
                .unwrap_or_default();

            TranscriptSegment {
                id: r.id,
                session_id: r.session_id,
                recording_id: r.recording_id,
                start_timestamp: r.start_timestamp,
                end_timestamp: r.end_timestamp,
                text: r.text,
                language: r.language,
                confidence: r.confidence as f32,
                speaker_id: r.speaker_id,
                words,
            }
        }).collect();

        Ok(segments)
    }
}
