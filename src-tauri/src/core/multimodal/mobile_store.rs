use crate::core::database::Database;
use std::sync::Arc;

/// Source id for audio arriving from the Source Mobile iOS app.
/// Timeline rails filter on this value; `audio_chunks` writers must
/// exclude it so mobile rows never leak into the Ambient lane.
pub const MOBILE_SOURCE_ID: &str = "source-mobile";

/// Persist a mobile transcript. `clip_id` doubles as `asr_segment_id` so
/// a full-file upload after a dropped live stream replaces the partial
/// row instead of duplicating it.
pub async fn persist_mobile_transcript(
    db: &Arc<Database>,
    session_id: &str,
    clip_id: &str,
    text: &str,
    started_at_ms: i64,
    ended_at_ms: i64,
    language: Option<&str>,
    confidence: Option<f32>,
    model: &str,
    is_final: bool,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO asr_segments (
            asr_segment_id, session_id, source_id, start_timestamp, end_timestamp,
            language, transcript, confidence, model_name, model_version, audio_chunk_ids_json,
            created_at, is_final
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(asr_segment_id) DO UPDATE SET
            end_timestamp = excluded.end_timestamp,
            language = excluded.language,
            transcript = excluded.transcript,
            confidence = excluded.confidence,
            audio_chunk_ids_json = excluded.audio_chunk_ids_json,
            created_at = excluded.created_at,
            is_final = excluded.is_final",
    )
    .bind(clip_id)
    .bind(session_id)
    .bind(MOBILE_SOURCE_ID)
    .bind(started_at_ms)
    .bind(ended_at_ms)
    .bind(language)
    .bind(text)
    .bind(confidence.map(f64::from))
    .bind(model)
    .bind("mobile-1")
    .bind("[]")
    .bind(chrono::Utc::now().timestamp_millis())
    .bind(i64::from(is_final))
    .execute(db.pool())
    .await
    .map_err(|error| format!("Failed to persist mobile transcript: {error}"))?;
    Ok(())
}

/// Track a spooled mobile clip on disk. Audio is retained outside the
/// desktop eviction path (Voice Memos replacement semantics).
pub async fn track_mobile_clip(
    db: &Arc<Database>,
    clip_id: &str,
    device_id: &str,
    started_at: i64,
    ended_at: i64,
    audio_path: &str,
    bytes: i64,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO mobile_clips (clip_id, device_id, started_at, ended_at, audio_path, bytes, delivered_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, NULL, ?)
         ON CONFLICT(clip_id) DO UPDATE SET
            ended_at = excluded.ended_at,
            audio_path = excluded.audio_path,
            bytes = excluded.bytes",
    )
    .bind(clip_id)
    .bind(device_id)
    .bind(started_at)
    .bind(ended_at)
    .bind(audio_path)
    .bind(bytes)
    .bind(chrono::Utc::now().timestamp_millis())
    .execute(db.pool())
    .await
    .map_err(|error| format!("Failed to track mobile clip: {error}"))?;
    Ok(())
}

pub async fn mark_mobile_clip_delivered(db: &Arc<Database>, clip_id: &str) -> Result<(), String> {
    sqlx::query("UPDATE mobile_clips SET delivered_at = ? WHERE clip_id = ?")
        .bind(chrono::Utc::now().timestamp_millis())
        .bind(clip_id)
        .execute(db.pool())
        .await
        .map_err(|error| format!("Failed to mark mobile clip delivered: {error}"))?;
    Ok(())
}

pub async fn mobile_clip_audio_path(
    db: &Arc<Database>,
    clip_id: &str,
) -> Result<Option<String>, String> {
    let row: Option<(String,)> = sqlx::query_as("SELECT audio_path FROM mobile_clips WHERE clip_id = ?")
        .bind(clip_id)
        .fetch_optional(db.pool())
        .await
        .map_err(|error| format!("Failed to look up mobile clip: {error}"))?;
    Ok(row.map(|row| row.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_source_id_is_stable() {
        assert_eq!(MOBILE_SOURCE_ID, "source-mobile");
    }
}
