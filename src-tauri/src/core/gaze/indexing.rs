use super::queries::get_attention_snapshots;
use super::types::{AttentionSnapshotDto, ATTENTION_MIN_DWELL_MS, ATTENTION_SPAN_MAX_GAP_MS};
use crate::core::database::Database;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub(crate) async fn reindex_attention_spans(
    db: &Arc<Database>,
    session_id: &str,
    source_id: &str,
) -> Result<(), String> {
    sqlx::query("DELETE FROM attention_spans WHERE session_id = ? AND source_id = ?")
        .bind(session_id)
        .bind(source_id)
        .execute(db.pool())
        .await
        .map_err(|e| format!("Failed to clear attention spans: {e}"))?;

    let snapshots = get_attention_snapshots(
        db,
        0,
        i64::MAX,
        Some(source_id.to_string()),
        Some(session_id.to_string()),
    )
    .await
    .map_err(|e| format!("Failed to load attention snapshots for reindex: {e}"))?;
    let mut active: Option<AttentionSpanSeed> = None;
    let mut seeds = Vec::new();

    for snapshot in snapshots {
        let Some(target) = snapshot.likely_targets.first() else {
            continue;
        };
        match active.as_mut() {
            Some(seed)
                if seed.target_type == target.target_type
                    && seed.target_id == target.target_id
                    && snapshot.timestamp - seed.last_seen_at <= ATTENTION_SPAN_MAX_GAP_MS =>
            {
                seed.last_seen_at = snapshot.timestamp;
                seed.snapshot_ids
                    .push(snapshot.attention_snapshot_id.clone());
                seed.confidences
                    .push(snapshot.confidence * target.probability);
            }
            _ => {
                if let Some(seed) = active.take() {
                    if should_persist_seed(&seed) {
                        seeds.push(seed);
                    }
                }
                active = Some(AttentionSpanSeed::from_snapshot(&snapshot));
            }
        }
    }

    if let Some(seed) = active.take() {
        if should_persist_seed(&seed) {
            seeds.push(seed);
        }
    }

    for seed in seeds {
        sqlx::query(
            "INSERT INTO attention_spans (
                attention_span_id, session_id, source_id, target_type, target_id, label,
                first_seen_at, last_seen_at, duration_ms, supporting_attention_snapshot_ids_json,
                avg_confidence, max_confidence, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(session_id)
        .bind(source_id)
        .bind(seed.target_type)
        .bind(seed.target_id)
        .bind(seed.label)
        .bind(seed.first_seen_at)
        .bind(seed.last_seen_at)
        .bind((seed.last_seen_at - seed.first_seen_at).max(ATTENTION_MIN_DWELL_MS))
        .bind(json!(seed.snapshot_ids).to_string())
        .bind(avg(&seed.confidences) as f64)
        .bind(seed.confidences.iter().copied().fold(0.0_f32, f32::max) as f64)
        .bind(chrono::Utc::now().timestamp_millis())
        .bind(chrono::Utc::now().timestamp_millis())
        .execute(db.pool())
        .await
        .map_err(|e| format!("Failed to insert attention span: {e}"))?;
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct AttentionSpanSeed {
    target_type: String,
    target_id: String,
    label: String,
    first_seen_at: i64,
    last_seen_at: i64,
    snapshot_ids: Vec<String>,
    confidences: Vec<f32>,
}

impl AttentionSpanSeed {
    fn from_snapshot(snapshot: &AttentionSnapshotDto) -> Self {
        let target = snapshot
            .likely_targets
            .first()
            .expect("attention span seed requires at least one target");
        Self {
            target_type: target.target_type.clone(),
            target_id: target.target_id.clone(),
            label: target.label.clone(),
            first_seen_at: snapshot.timestamp,
            last_seen_at: snapshot.timestamp,
            snapshot_ids: vec![snapshot.attention_snapshot_id.clone()],
            confidences: vec![snapshot.confidence * target.probability],
        }
    }
}

fn should_persist_seed(seed: &AttentionSpanSeed) -> bool {
    !seed.snapshot_ids.is_empty()
        && ((seed.last_seen_at - seed.first_seen_at).max(ATTENTION_MIN_DWELL_MS)
            >= ATTENTION_MIN_DWELL_MS)
}

fn avg(values: &[f32]) -> f32 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f32>() / values.len() as f32
    }
}
