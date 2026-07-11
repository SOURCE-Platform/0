use super::audio_intelligence_queries::get_sound_event_detections;
use super::audio_intelligence_types::SoundEventSpanSeed;
use crate::core::database::Database;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

const SOUND_EVENT_SPAN_MAX_GAP_MS: i64 = 4_000;
const SOUND_EVENT_MIN_CONFIDENCE: f32 = 0.35;

pub(super) async fn reindex_sound_event_spans(
    db: &Arc<Database>,
    session_id: &str,
    source_id: &str,
) -> Result<(), String> {
    let detections = get_sound_event_detections(db, 0, i64::MAX, Some(source_id.to_string()))
        .await
        .map_err(|e| format!("Failed to load sound event detections for reindex: {e}"))?
        .into_iter()
        .filter(|item| item.session_id == session_id)
        .collect::<Vec<_>>();

    sqlx::query("DELETE FROM sound_event_spans WHERE session_id = ? AND source_id = ?")
        .bind(session_id)
        .bind(source_id)
        .execute(db.pool())
        .await
        .map_err(|e| format!("Failed to clear sound event spans: {e}"))?;

    let mut seeds: Vec<SoundEventSpanSeed> = Vec::new();
    let mut current: Option<SoundEventSpanSeed> = None;

    for detection in detections {
        if detection.confidence < SOUND_EVENT_MIN_CONFIDENCE {
            continue;
        }

        match current.as_mut() {
            Some(active)
                if active.canonical_label == detection.canonical_label
                    && active.model_name == detection.model_name
                    && detection.start_timestamp - active.last_seen_at
                        <= SOUND_EVENT_SPAN_MAX_GAP_MS =>
            {
                active.last_seen_at = detection.end_timestamp;
                active
                    .supporting_detection_ids
                    .push(detection.sound_event_detection_id.clone());
                active
                    .supporting_audio_chunk_ids
                    .push(detection.audio_chunk_id.clone());
                active.confidences.push(detection.confidence);
            }
            _ => {
                if let Some(active) = current.take() {
                    seeds.push(active);
                }
                current = Some(SoundEventSpanSeed {
                    canonical_label: detection.canonical_label.clone(),
                    first_seen_at: detection.start_timestamp,
                    last_seen_at: detection.end_timestamp,
                    supporting_detection_ids: vec![detection.sound_event_detection_id.clone()],
                    supporting_audio_chunk_ids: vec![detection.audio_chunk_id.clone()],
                    confidences: vec![detection.confidence],
                    model_name: detection.model_name.clone(),
                    model_version: detection.model_version.clone(),
                });
            }
        }
    }

    if let Some(active) = current.take() {
        seeds.push(active);
    }

    for seed in seeds {
        sqlx::query(
            "INSERT INTO sound_event_spans (
                sound_event_span_id, session_id, source_id, canonical_label, first_seen_at,
                last_seen_at, duration_ms, supporting_detection_ids_json,
                supporting_audio_chunk_ids_json, avg_confidence, max_confidence, model_name,
                model_version, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(session_id)
        .bind(source_id)
        .bind(seed.canonical_label)
        .bind(seed.first_seen_at)
        .bind(seed.last_seen_at)
        .bind((seed.last_seen_at - seed.first_seen_at).max(1_000))
        .bind(json!(seed.supporting_detection_ids).to_string())
        .bind(json!(seed.supporting_audio_chunk_ids).to_string())
        .bind(avg_confidence(&seed.confidences) as f64)
        .bind(max_confidence(&seed.confidences) as f64)
        .bind(seed.model_name)
        .bind(seed.model_version)
        .bind(chrono::Utc::now().timestamp_millis())
        .bind(chrono::Utc::now().timestamp_millis())
        .execute(db.pool())
        .await
        .map_err(|e| format!("Failed to insert sound event span: {e}"))?;
    }

    Ok(())
}

fn avg_confidence(values: &[f32]) -> f32 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f32>() / values.len() as f32
    }
}

fn max_confidence(values: &[f32]) -> f32 {
    values.iter().copied().reduce(f32::max).unwrap_or(0.0)
}
