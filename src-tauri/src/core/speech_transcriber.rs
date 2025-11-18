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
        #[cfg(feature = "ml-pyo3")]
        {
            self.transcribe_with_pyo3(session_id, recording_id, audio_file_path).await
        }

        #[cfg(not(feature = "ml-pyo3"))]
        {
            println!("Whisper transcription not available (enable 'ml-pyo3' feature)");
            println!("Audio file: {}", audio_file_path);
            Ok(vec![])
        }
    }

    #[cfg(feature = "ml-pyo3")]
    async fn transcribe_with_pyo3(
        &self,
        session_id: &str,
        recording_id: &str,
        audio_file_path: &str,
    ) -> AudioResult<Vec<TranscriptSegment>> {
        use pyo3::prelude::*;
        use pyo3::types::PyDict;

        let model_size = self.model_size.read().await.clone();
        let language = self.language.read().await.clone();

        // Run Python inference in blocking task (Whisper is CPU/GPU intensive)
        let audio_path = audio_file_path.to_string();
        let session_id = session_id.to_string();
        let recording_id = recording_id.to_string();

        let result = tokio::task::spawn_blocking(move || {
            Python::with_gil(|py| {
                // Add python directory to sys.path
                let sys = py.import("sys")
                    .map_err(|e| AudioError::TranscriptionFailed(format!("Failed to import sys: {}", e)))?;

                let path_list = sys.getattr("path")
                    .map_err(|e| AudioError::TranscriptionFailed(format!("Failed to get sys.path: {}", e)))?;

                let python_dir = std::env::current_dir()
                    .unwrap_or_default()
                    .join("src-tauri")
                    .join("python");

                path_list.call_method1("insert", (0, python_dir.to_str().unwrap()))
                    .map_err(|e| AudioError::TranscriptionFailed(format!("Failed to add python dir: {}", e)))?;

                // Import whisper_inference module
                let whisper_module = py.import("whisper_inference")
                    .map_err(|e| AudioError::TranscriptionFailed(format!(
                        "Failed to import whisper_inference: {}. Install with: pip install -r python/requirements.txt",
                        e
                    )))?;

                // Call transcribe_file function
                let transcribe_fn = whisper_module.getattr("transcribe_file")
                    .map_err(|e| AudioError::TranscriptionFailed(format!("Failed to get transcribe_file: {}", e)))?;

                // Prepare arguments
                let kwargs = PyDict::new(py);
                kwargs.set_item("audio_path", &audio_path)
                    .map_err(|e| AudioError::TranscriptionFailed(format!("Failed to set audio_path: {}", e)))?;
                kwargs.set_item("model_size", model_size.to_string())
                    .map_err(|e| AudioError::TranscriptionFailed(format!("Failed to set model_size: {}", e)))?;

                if let Some(lang) = language {
                    kwargs.set_item("language", lang)
                        .map_err(|e| AudioError::TranscriptionFailed(format!("Failed to set language: {}", e)))?;
                }

                kwargs.set_item("word_timestamps", true)
                    .map_err(|e| AudioError::TranscriptionFailed(format!("Failed to set word_timestamps: {}", e)))?;

                println!("Running Whisper transcription on: {}", audio_path);

                // Call the function
                let result_json = transcribe_fn.call((), Some(kwargs))
                    .map_err(|e| AudioError::TranscriptionFailed(format!("Whisper inference failed: {}", e)))?;

                // Extract JSON string
                let json_str: String = result_json.extract()
                    .map_err(|e| AudioError::TranscriptionFailed(format!("Failed to extract JSON: {}", e)))?;

                // Parse JSON result
                let result: serde_json::Value = serde_json::from_str(&json_str)
                    .map_err(|e| AudioError::TranscriptionFailed(format!("Failed to parse JSON: {}", e)))?;

                // Extract segments
                let segments_data = result.get("segments")
                    .and_then(|s| s.as_array())
                    .ok_or_else(|| AudioError::TranscriptionFailed("Missing segments in result".to_string()))?;

                let language_detected = result.get("language")
                    .and_then(|l| l.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                let segments: Vec<TranscriptSegment> = segments_data.iter()
                    .map(|seg| {
                        let start = (seg.get("start").and_then(|v| v.as_f64()).unwrap_or(0.0) * 1000.0) as i64;
                        let end = (seg.get("end").and_then(|v| v.as_f64()).unwrap_or(0.0) * 1000.0) as i64;
                        let text = seg.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
                        let confidence = seg.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.0) as f32;

                        // Extract word timestamps
                        let words = if let Some(words_data) = seg.get("words").and_then(|w| w.as_array()) {
                            words_data.iter()
                                .map(|word| WordTimestamp {
                                    word: word.get("word").and_then(|w| w.as_str()).unwrap_or("").to_string(),
                                    start_timestamp: (word.get("start").and_then(|v| v.as_f64()).unwrap_or(0.0) * 1000.0) as i64,
                                    end_timestamp: (word.get("end").and_then(|v| v.as_f64()).unwrap_or(0.0) * 1000.0) as i64,
                                    probability: word.get("probability").and_then(|p| p.as_f64()).unwrap_or(1.0) as f32,
                                })
                                .collect()
                        } else {
                            vec![]
                        };

                        TranscriptSegment {
                            id: Uuid::new_v4().to_string(),
                            session_id: session_id.clone(),
                            recording_id: recording_id.clone(),
                            start_timestamp: start,
                            end_timestamp: end,
                            text,
                            language: language_detected.clone(),
                            confidence,
                            speaker_id: None, // Will be populated by diarization
                            words,
                        }
                    })
                    .collect();

                println!("Transcription complete: {} segments", segments.len());

                Ok::<Vec<TranscriptSegment>, AudioError>(segments)
            })
        })
        .await
        .map_err(|e| AudioError::TranscriptionFailed(format!("Tokio join error: {}", e)))??;

        // Store all segments in database
        for segment in &result {
            self.store_transcript(segment).await?;
        }

        Ok(result)
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
