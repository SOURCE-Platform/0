use super::types::{
    AttentionAtTimestampDto, AttentionSearchResultDto, AttentionSnapshotDto, AttentionSnapshotRow,
    AttentionSpanDto, AttentionSpanRow, AttentionSummaryDto, AttentionTargetDto, GazeSampleDto,
    GazeSampleRow,
};
use crate::core::database::Database;
use crate::core::ocr_agent_context;
use std::collections::HashMap;
use std::sync::Arc;

pub async fn get_gaze_samples(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
    session_filter: Option<String>,
) -> Result<Vec<GazeSampleDto>, String> {
    let mut query =
        String::from("SELECT * FROM gaze_samples WHERE timestamp >= ? AND timestamp <= ?");
    if source_filter.is_some() {
        query.push_str(" AND source_id = ?");
    }
    if session_filter.is_some() {
        query.push_str(" AND session_id = ?");
    }
    query.push_str(" ORDER BY timestamp ASC");

    let mut built = sqlx::query_as::<_, GazeSampleRow>(&query)
        .bind(start_timestamp)
        .bind(end_timestamp);
    if let Some(source_filter) = source_filter {
        built = built.bind(source_filter);
    }
    if let Some(session_filter) = session_filter {
        built = built.bind(session_filter);
    }
    built
        .fetch_all(db.pool())
        .await
        .map_err(|e| format!("Failed to load gaze samples: {e}"))?
        .into_iter()
        .map(map_gaze_sample)
        .collect()
}

pub async fn get_attention_snapshots(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
    session_filter: Option<String>,
) -> Result<Vec<AttentionSnapshotDto>, String> {
    let mut query =
        String::from("SELECT * FROM attention_snapshots WHERE timestamp >= ? AND timestamp <= ?");
    if source_filter.is_some() {
        query.push_str(" AND source_id = ?");
    }
    if session_filter.is_some() {
        query.push_str(" AND session_id = ?");
    }
    query.push_str(" ORDER BY timestamp ASC");

    let mut built = sqlx::query_as::<_, AttentionSnapshotRow>(&query)
        .bind(start_timestamp)
        .bind(end_timestamp);
    if let Some(source_filter) = source_filter {
        built = built.bind(source_filter);
    }
    if let Some(session_filter) = session_filter {
        built = built.bind(session_filter);
    }
    built
        .fetch_all(db.pool())
        .await
        .map_err(|e| format!("Failed to load attention snapshots: {e}"))?
        .into_iter()
        .map(map_attention_snapshot)
        .collect()
}

pub async fn get_attention_spans(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
) -> Result<Vec<AttentionSpanDto>, String> {
    let rows = if let Some(source_filter) = source_filter {
        sqlx::query_as::<_, AttentionSpanRow>(
            "SELECT * FROM attention_spans
             WHERE first_seen_at <= ? AND last_seen_at >= ? AND source_id = ?
             ORDER BY first_seen_at ASC",
        )
        .bind(end_timestamp)
        .bind(start_timestamp)
        .bind(source_filter)
        .fetch_all(db.pool())
        .await
    } else {
        sqlx::query_as::<_, AttentionSpanRow>(
            "SELECT * FROM attention_spans
             WHERE first_seen_at <= ? AND last_seen_at >= ?
             ORDER BY first_seen_at ASC",
        )
        .bind(end_timestamp)
        .bind(start_timestamp)
        .fetch_all(db.pool())
        .await
    }
    .map_err(|e| format!("Failed to load attention spans: {e}"))?;

    rows.into_iter().map(map_attention_span).collect()
}

pub async fn get_attention_at_timestamp(
    db: &Arc<Database>,
    timestamp: i64,
) -> Result<AttentionAtTimestampDto, String> {
    let samples = get_gaze_samples(
        db,
        timestamp.saturating_sub(30_000),
        timestamp.saturating_add(30_000),
        None,
        None,
    )
    .await?;
    let snapshots = get_attention_snapshots(
        db,
        timestamp.saturating_sub(30_000),
        timestamp.saturating_add(30_000),
        None,
        None,
    )
    .await?;

    Ok(AttentionAtTimestampDto {
        timestamp,
        sample: samples
            .into_iter()
            .min_by_key(|item| (item.timestamp - timestamp).abs()),
        snapshot: snapshots
            .into_iter()
            .min_by_key(|item| (item.timestamp - timestamp).abs()),
        scene: ocr_agent_context::get_activity_episode(db, timestamp)
            .await
            .ok()
            .and_then(|episode| episode.scene),
        text_spans: ocr_agent_context::get_text_spans(
            db,
            timestamp.saturating_sub(60_000),
            timestamp.saturating_add(60_000),
            None,
        )
        .await
        .map_err(|e| format!("Failed to load OCR text spans: {e}"))?,
        context_entity: ocr_agent_context::get_context_entities(
            db,
            timestamp.saturating_sub(180_000),
            timestamp.saturating_add(180_000),
            None,
            None,
        )
        .await
        .map_err(|e| format!("Failed to load OCR context entities: {e}"))?
        .into_iter()
        .find(|entity| entity.first_seen_at <= timestamp && entity.last_seen_at >= timestamp),
    })
}

pub async fn get_attention_summary(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<AttentionSummaryDto, String> {
    let samples = get_gaze_samples(db, start_timestamp, end_timestamp, None, None).await?;
    let snapshots = get_attention_snapshots(db, start_timestamp, end_timestamp, None, None).await?;
    let spans = get_attention_spans(db, start_timestamp, end_timestamp, None).await?;
    let calibration_quality = sqlx::query_scalar::<_, String>(
        "SELECT validation_quality FROM gaze_calibrations WHERE active = 1 ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_optional(db.pool())
    .await
    .map_err(|e| format!("Failed to load active calibration quality: {e}"))?;

    let mut label_counts = HashMap::<String, usize>::new();
    for span in &spans {
        *label_counts.entry(span.label.clone()).or_default() += 1;
    }

    Ok(AttentionSummaryDto {
        start_timestamp,
        end_timestamp,
        calibration_quality,
        gaze_sample_count: samples.len(),
        attention_snapshot_count: snapshots.len(),
        attention_span_count: spans.len(),
        top_targets: top_labels(label_counts),
    })
}

pub async fn search_attention_context(
    db: &Arc<Database>,
    query: String,
    start_timestamp: Option<i64>,
    end_timestamp: Option<i64>,
    source_filter: Option<String>,
) -> Result<Vec<AttentionSearchResultDto>, String> {
    let spans = get_attention_spans(
        db,
        start_timestamp.unwrap_or(0),
        end_timestamp.unwrap_or(i64::MAX),
        source_filter,
    )
    .await?;
    let lowered = query.to_lowercase();
    Ok(spans
        .into_iter()
        .filter(|span| span.label.to_lowercase().contains(&lowered))
        .map(|span| AttentionSearchResultDto {
            target_type: span.target_type,
            target_id: span.target_id,
            label: span.label,
            timestamp: span.first_seen_at,
            duration_ms: span.duration_ms,
            confidence: span.avg_confidence,
        })
        .collect())
}

pub async fn delete_all_gaze_data(db: &Arc<Database>) -> Result<(), String> {
    for query in [
        "DELETE FROM attention_spans",
        "DELETE FROM attention_snapshots",
        "DELETE FROM gaze_samples",
        "DELETE FROM gaze_calibrations",
    ] {
        sqlx::query(query)
            .execute(db.pool())
            .await
            .map_err(|e| format!("Failed to delete gaze data: {e}"))?;
    }
    Ok(())
}

fn map_gaze_sample(row: GazeSampleRow) -> Result<GazeSampleDto, String> {
    Ok(GazeSampleDto {
        gaze_sample_id: row.gaze_sample_id,
        session_id: row.session_id,
        timestamp: row.timestamp,
        calibration_id: row.calibration_id,
        source_id: row.source_id,
        screen_x: row.screen_x as f32,
        screen_y: row.screen_y as f32,
        confidence: row.confidence as f32,
        accuracy_radius_px: row.accuracy_radius_px as f32,
        head_pose: parse_optional_json(row.head_pose_json)?,
        face_bbox: parse_optional_json(row.face_bbox_json)?,
        gaze_vector: parse_optional_json(row.gaze_vector_json)?,
        projected_point: parse_optional_json(row.projected_point_json)?,
        landmark_payload: parse_optional_json(row.landmark_payload_json)?,
        raw_features_ref: row.raw_features_ref,
        model_name: row.model_name,
        model_version: row.model_version,
    })
}

fn map_attention_snapshot(row: AttentionSnapshotRow) -> Result<AttentionSnapshotDto, String> {
    Ok(AttentionSnapshotDto {
        attention_snapshot_id: row.attention_snapshot_id,
        session_id: row.session_id,
        timestamp: row.timestamp,
        gaze_sample_id: row.gaze_sample_id,
        source_id: row.source_id,
        screen_x: row.screen_x as f32,
        screen_y: row.screen_y as f32,
        accuracy_radius_px: row.accuracy_radius_px as f32,
        confidence: row.confidence as f32,
        frontmost_app_name: row.frontmost_app_name,
        window_title: row.window_title,
        likely_targets: serde_json::from_str::<Vec<AttentionTargetDto>>(&row.likely_targets_json)
            .map_err(|e| format!("Failed to parse attention targets: {e}"))?,
        resolver_version: row.resolver_version,
    })
}

fn map_attention_span(row: AttentionSpanRow) -> Result<AttentionSpanDto, String> {
    Ok(AttentionSpanDto {
        attention_span_id: row.attention_span_id,
        session_id: row.session_id,
        source_id: row.source_id,
        target_type: row.target_type,
        target_id: row.target_id,
        label: row.label,
        first_seen_at: row.first_seen_at,
        last_seen_at: row.last_seen_at,
        duration_ms: row.duration_ms,
        supporting_attention_snapshot_ids: serde_json::from_str(
            &row.supporting_attention_snapshot_ids_json,
        )
        .map_err(|e| format!("Failed to parse attention span snapshot ids: {e}"))?,
        avg_confidence: row.avg_confidence as f32,
        max_confidence: row.max_confidence as f32,
    })
}

fn parse_optional_json(input: Option<String>) -> Result<Option<serde_json::Value>, String> {
    input
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| format!("Failed to parse gaze JSON payload: {e}"))
}

fn top_labels(counts: HashMap<String, usize>) -> Vec<String> {
    let mut pairs = counts.into_iter().collect::<Vec<_>>();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    pairs.into_iter().take(6).map(|(label, _)| label).collect()
}
