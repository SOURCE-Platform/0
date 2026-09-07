use crate::core::database::Database;
use std::sync::Arc;

/// Timeline write path for foreground dictations. The transcript id doubles
/// as the segment id, so retried deliveries deduplicate in the database
/// exactly as they do in memory (`DictationPipeline::seen_transcript_ids`).
pub const DICTATION_SOURCE_ID: &str = "fluid-voice-prompt";

pub async fn persist_foreground_transcript(
    db: &Arc<Database>,
    session_id: &str,
    id: &str,
    text: &str,
    started_at_ms: i64,
    ended_at_ms: i64,
    language: Option<&str>,
    confidence: Option<f32>,
    model: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO asr_segments (
            asr_segment_id, session_id, source_id, start_timestamp, end_timestamp,
            language, transcript, confidence, model_name, model_version, audio_chunk_ids_json,
            created_at, is_final
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(asr_segment_id) DO NOTHING",
    )
    .bind(id)
    .bind(session_id)
    .bind(DICTATION_SOURCE_ID)
    .bind(started_at_ms)
    .bind(ended_at_ms)
    .bind(language)
    .bind(text)
    .bind(confidence.map(f64::from))
    .bind(model)
    .bind("helper-1")
    .bind("[]")
    .bind(chrono::Utc::now().timestamp_millis())
    .bind(1_i64)
    .execute(db.pool())
    .await
    .map_err(|error| format!("Failed to persist dictation transcript: {error}"))?;
    Ok(())
}
