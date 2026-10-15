use super::constants::{
    AUDIO_MIN_SPEECH_CONFIDENCE, AUDIO_SILENCE_HANGOVER_MS, AUDIO_SPAN_MAX_GAP_MS,
    VISUAL_CAPTURE_INTERVAL_MS, VISUAL_EXIT_POSTURE_CONFIDENCE, VISUAL_MIN_POSTURE_CONFIDENCE,
    VISUAL_SPAN_MAX_GAP_MS,
};
use super::queries::get_visual_scene_snapshots;
use super::types::{AudioSpanSeed, VisualSceneSnapshotDto, VisualSpanSeed};
use crate::core::database::Database;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub(super) async fn reindex_visual_state_spans(
    db: &Arc<Database>,
    session_id: &str,
    source_id: &str,
) -> Result<(), String> {
    let scenes = get_visual_scene_snapshots(db, 0, i64::MAX, Some(source_id.to_string()))
        .await
        .map_err(|e| format!("Failed to load visual scenes for reindex: {e}"))?
        .into_iter()
        .filter(|scene| scene.session_id == session_id)
        .collect::<Vec<_>>();

    sqlx::query("DELETE FROM visual_state_spans WHERE session_id = ? AND source_id = ?")
        .bind(session_id)
        .bind(source_id)
        .execute(db.pool())
        .await
        .map_err(|e| format!("Failed to clear visual spans: {e}"))?;

    let seeds = [
        build_visual_span_seeds(&scenes, "presence"),
        build_visual_span_seeds(&scenes, "posture"),
        build_visual_span_seeds(&scenes, "motion"),
    ]
    .concat();

    for seed in seeds {
        sqlx::query(
            "INSERT INTO visual_state_spans (
                visual_state_span_id, session_id, source_id, state_type, label, first_seen_at,
                last_seen_at, duration_ms, scene_ids_json, avg_confidence, min_confidence,
                max_confidence, transition_in, transition_out, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(session_id)
        .bind(source_id)
        .bind(seed.state_type)
        .bind(seed.label)
        .bind(seed.first_seen_at)
        .bind(seed.last_seen_at)
        .bind((seed.last_seen_at - seed.first_seen_at).max(VISUAL_CAPTURE_INTERVAL_MS))
        .bind(json!(seed.scene_ids).to_string())
        .bind(avg_confidence(&seed.confidences) as f64)
        .bind(min_confidence(&seed.confidences) as f64)
        .bind(max_confidence(&seed.confidences) as f64)
        .bind(seed.transition_in)
        .bind(seed.transition_out)
        .bind(chrono::Utc::now().timestamp_millis())
        .bind(chrono::Utc::now().timestamp_millis())
        .execute(db.pool())
        .await
        .map_err(|e| format!("Failed to insert visual span: {e}"))?;
    }

    Ok(())
}

pub(super) async fn reindex_audio_state_spans(
    db: &Arc<Database>,
    session_id: &str,
    source_id: &str,
    timestamp: i64,
) -> Result<(), String> {
    // Rebuild just the five-minute slice touched by the new chunk. Loading
    // every waveform in a long session on each two-second chunk exhausted CPU.
    const BUCKET_MS: i64 = 300_000;
    let bucket_start = timestamp.div_euclid(BUCKET_MS) * BUCKET_MS;
    let bucket_end = bucket_start + BUCKET_MS;
    let chunks = sqlx::query_as::<_, (String, i64, i64, f64, i64)>(
        "SELECT audio_chunk_id, start_timestamp, end_timestamp, vad_score, speech_detected
         FROM audio_chunks INDEXED BY idx_audio_chunks_time
         WHERE start_timestamp >= ? AND start_timestamp < ? AND session_id = ? AND source_id = ?
         ORDER BY start_timestamp",
    )
    .bind(bucket_start)
    .bind(bucket_end)
    .bind(session_id)
    .bind(source_id)
    .fetch_all(db.pool())
    .await
    .map_err(|e| format!("Failed to load recent audio chunks: {e}"))?;
    let asr_segments = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT asr_segment_id, start_timestamp, end_timestamp
         FROM asr_segments INDEXED BY idx_asr_segments_time
         WHERE start_timestamp < ? AND end_timestamp >= ? AND session_id = ? AND source_id = ?",
    )
    .bind(bucket_end)
    .bind(bucket_start)
    .bind(session_id)
    .bind(source_id)
    .fetch_all(db.pool())
    .await
    .map_err(|e| format!("Failed to load recent ASR segments: {e}"))?;

    // A span from an older app build can cross this bucket boundary. Keep
    // its earlier time range and replace only the recent part below.
    sqlx::query(
        "UPDATE audio_state_spans SET last_seen_at = ?, duration_ms = MAX(0, ? - first_seen_at), updated_at = ?
         WHERE session_id = ? AND source_id = ? AND first_seen_at < ? AND last_seen_at >= ?",
    )
    .bind(bucket_start)
    .bind(bucket_start)
    .bind(chrono::Utc::now().timestamp_millis())
    .bind(session_id)
    .bind(source_id)
    .bind(bucket_start)
    .bind(bucket_start)
    .execute(db.pool())
    .await
    .map_err(|e| format!("Failed to preserve earlier audio span: {e}"))?;
    sqlx::query(
        "DELETE FROM audio_state_spans WHERE session_id = ? AND source_id = ?
         AND first_seen_at >= ? AND first_seen_at < ?",
    )
    .bind(session_id)
    .bind(source_id)
    .bind(bucket_start)
    .bind(bucket_end)
    .execute(db.pool())
    .await
    .map_err(|e| format!("Failed to clear audio spans: {e}"))?;

    let mut seeds: Vec<AudioSpanSeed> = Vec::new();
    let mut current: Option<AudioSpanSeed> = None;

    for (audio_chunk_id, start_timestamp, end_timestamp, vad_score, speech_detected) in chunks {
        let vad_score = vad_score as f32;
        let label = if speech_detected != 0 {
            if vad_score >= 0.25 {
                "speaking"
            } else {
                "intermittent_speech"
            }
        } else {
            "silent"
        };
        let supporting_asr_segment_ids = asr_segments
            .iter()
            .filter(|segment| segment.1 <= end_timestamp && segment.2 >= start_timestamp)
            .map(|segment| segment.0.clone())
            .collect::<Vec<_>>();

        match current.as_mut() {
            Some(active)
                if active.label == label
                    && start_timestamp - active.last_seen_at <= AUDIO_SPAN_MAX_GAP_MS =>
            {
                active.last_seen_at = end_timestamp;
                active
                    .supporting_audio_chunk_ids
                    .push(audio_chunk_id.clone());
                active
                    .supporting_asr_segment_ids
                    .extend(supporting_asr_segment_ids.clone());
                active.confidences.push(vad_score);
            }
            _ => {
                if let Some(active) = current.take() {
                    if should_persist_audio_seed(&active) {
                        seeds.push(active);
                    }
                }
                current = Some(AudioSpanSeed {
                    label: label.to_string(),
                    first_seen_at: start_timestamp,
                    last_seen_at: end_timestamp,
                    supporting_audio_chunk_ids: vec![audio_chunk_id.clone()],
                    supporting_asr_segment_ids,
                    confidences: vec![vad_score],
                });
            }
        }
    }

    if let Some(active) = current {
        if should_persist_audio_seed(&active) {
            seeds.push(active);
        }
    }

    for seed in prune_audio_silence(seeds) {
        sqlx::query(
            "INSERT INTO audio_state_spans (
                audio_state_span_id, session_id, source_id, state_type, label, first_seen_at,
                last_seen_at, duration_ms, supporting_audio_chunk_ids_json,
                supporting_asr_segment_ids_json, avg_confidence, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(session_id)
        .bind(source_id)
        .bind("speech")
        .bind(seed.label)
        .bind(seed.first_seen_at)
        .bind(seed.last_seen_at)
        .bind((seed.last_seen_at - seed.first_seen_at).max(AUDIO_SILENCE_HANGOVER_MS))
        .bind(json!(seed.supporting_audio_chunk_ids).to_string())
        .bind(json!(seed.supporting_asr_segment_ids).to_string())
        .bind(avg_confidence(&seed.confidences) as f64)
        .bind(chrono::Utc::now().timestamp_millis())
        .bind(chrono::Utc::now().timestamp_millis())
        .execute(db.pool())
        .await
        .map_err(|e| format!("Failed to insert audio span: {e}"))?;
    }

    Ok(())
}

fn build_visual_span_seeds(
    scenes: &[VisualSceneSnapshotDto],
    state_type: &str,
) -> Vec<VisualSpanSeed> {
    let mut seeds = Vec::new();
    let mut current: Option<VisualSpanSeed> = None;

    for scene in scenes {
        let (raw_label, raw_confidence) = match state_type {
            "presence" => (scene.presence_label.clone(), scene.presence_confidence),
            "posture" => (scene.posture_label.clone(), scene.posture_confidence),
            "motion" => (scene.motion_label.clone(), scene.motion_confidence),
            _ => continue,
        };

        let label = match state_type {
            "posture" if raw_confidence < VISUAL_MIN_POSTURE_CONFIDENCE => "unknown".to_string(),
            "presence" if raw_confidence < VISUAL_EXIT_POSTURE_CONFIDENCE => "unknown".to_string(),
            "motion" if raw_confidence < VISUAL_EXIT_POSTURE_CONFIDENCE => "unknown".to_string(),
            _ => raw_label,
        };

        match current.as_mut() {
            Some(active)
                if active.label == label
                    && scene.timestamp - active.last_seen_at <= VISUAL_SPAN_MAX_GAP_MS =>
            {
                active.last_seen_at = scene.timestamp + VISUAL_CAPTURE_INTERVAL_MS;
                active.scene_ids.push(scene.visual_scene_id.clone());
                active.confidences.push(raw_confidence);
            }
            _ => {
                if let Some(active) = current.take() {
                    if should_persist_visual_seed(&active) {
                        seeds.push(active);
                    }
                }
                current = Some(VisualSpanSeed {
                    state_type: state_type.to_string(),
                    label,
                    first_seen_at: scene.timestamp,
                    last_seen_at: scene.timestamp + VISUAL_CAPTURE_INTERVAL_MS,
                    scene_ids: vec![scene.visual_scene_id.clone()],
                    confidences: vec![raw_confidence],
                    transition_in: None,
                    transition_out: None,
                });
            }
        }
    }

    if let Some(active) = current {
        if should_persist_visual_seed(&active) {
            seeds.push(active);
        }
    }
    seeds
}

fn should_persist_visual_seed(seed: &VisualSpanSeed) -> bool {
    if seed.state_type == "posture" {
        return seed.scene_ids.len() >= 1 && (seed.last_seen_at - seed.first_seen_at) >= 2_000;
    }
    true
}

fn should_persist_audio_seed(seed: &AudioSpanSeed) -> bool {
    if seed.label == "silent" {
        return (seed.last_seen_at - seed.first_seen_at) >= AUDIO_SILENCE_HANGOVER_MS;
    }
    avg_confidence(&seed.confidences) >= AUDIO_MIN_SPEECH_CONFIDENCE
}

fn prune_audio_silence(seeds: Vec<AudioSpanSeed>) -> Vec<AudioSpanSeed> {
    let keep_flags = seeds
        .iter()
        .enumerate()
        .map(|(index, seed)| {
            if seed.label != "silent" {
                return true;
            }

            let duration_ms = seed.last_seen_at - seed.first_seen_at;
            let has_previous_speech = seeds[..index]
                .iter()
                .rev()
                .any(|candidate| candidate.label != "silent");
            let has_next_speech = seeds[index + 1..]
                .iter()
                .any(|candidate| candidate.label != "silent");

            duration_ms >= 15_000 || (has_previous_speech && has_next_speech)
        })
        .collect::<Vec<_>>();

    seeds
        .into_iter()
        .zip(keep_flags)
        .filter_map(|(seed, keep)| keep.then_some(seed))
        .collect()
}

fn avg_confidence(values: &[f32]) -> f32 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f32>() / values.len() as f32
    }
}

fn min_confidence(values: &[f32]) -> f32 {
    values.iter().copied().reduce(f32::min).unwrap_or(0.0)
}

fn max_confidence(values: &[f32]) -> f32 {
    values.iter().copied().reduce(f32::max).unwrap_or(0.0)
}
