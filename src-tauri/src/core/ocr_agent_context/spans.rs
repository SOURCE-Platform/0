use super::entities::{build_context_entities, insert_context_entity};
use super::mappings::scene_row_to_dto;
use super::pii::{
    bbox_is_continuous, detect_pii_entities, normalize_text, union_bbox, unique_strings,
};
use super::{
    AgentSceneSnapshotDto, AgentTextSpanDto, MutableTextSpan, SceneSnapshotRow, SPAN_MAX_GAP_MS,
};
use crate::core::database::Database;
use std::sync::Arc;

pub(super) async fn rebuild_session_spans_and_entities(
    db: &Arc<Database>,
    session_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let scenes = sqlx::query_as::<_, SceneSnapshotRow>(
        r#"
        SELECT scene_id, session_id, timestamp, display_id, frontmost_app_name, frontmost_bundle_id,
               window_title, trigger_reason, frame_path, frame_width, frame_height, full_text,
               avg_confidence, block_count, text_blocks_json, pii_entities_json, linked_context_json,
               raw_source_json
        FROM ocr_scene_snapshots
        WHERE session_id = ?
        ORDER BY timestamp ASC
        "#,
    )
    .bind(session_id)
    .fetch_all(db.pool())
    .await?
    .into_iter()
    .map(scene_row_to_dto)
    .collect::<Result<Vec<_>, _>>()?;

    sqlx::query("DELETE FROM ocr_text_spans WHERE session_id = ?")
        .bind(session_id)
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM ocr_context_entities WHERE session_id = ?")
        .bind(session_id)
        .execute(db.pool())
        .await?;

    let spans = build_text_spans(session_id, &scenes);
    for span in &spans {
        insert_text_span(db, span).await?;
    }
    for entity in &build_context_entities(session_id, &scenes, &spans) {
        insert_context_entity(db, entity).await?;
    }

    Ok(())
}

pub(super) fn build_text_spans(
    session_id: &str,
    scenes: &[AgentSceneSnapshotDto],
) -> Vec<AgentTextSpanDto> {
    let mut spans: Vec<MutableTextSpan> = Vec::new();

    for scene in scenes {
        for block in &scene.text_blocks {
            let normalized = normalize_text(&block.text);
            if normalized.is_empty() {
                continue;
            }

            let matched_index = spans.iter().position(|span| {
                span.normalized_text == normalized
                    && span.frontmost_app_name == scene.frontmost_app_name
                    && span.frontmost_bundle_id == scene.frontmost_bundle_id
                    && span.window_title == scene.window_title
                    && scene.timestamp - span.last_seen_at <= SPAN_MAX_GAP_MS
                    && bbox_is_continuous(&span.bbox_union, &block.bbox)
            });

            if let Some(index) = matched_index {
                let span = &mut spans[index];
                span.last_seen_at = scene.timestamp;
                span.scene_ids.push(scene.scene_id.clone());
                span.confidence_sum += block.confidence;
                span.confidence_count += 1;
                span.bbox_union = union_bbox(&span.bbox_union, &block.bbox);
                span.occurrence_count += 1;
                span.raw_row_ids.push(block.block_id.clone());
                if span.canonical_text != block.text {
                    span.was_partial_match = true;
                    if block.text.len() > span.canonical_text.len() {
                        span.canonical_text = block.text.clone();
                    }
                }
                let new_entities = detect_pii_entities(&block.text, Some(block.bbox.clone()))
                    .into_iter()
                    .filter(|candidate| {
                        !span.pii_entities.iter().any(|existing| {
                            existing.entity_type == candidate.entity_type
                                && existing.redacted_preview == candidate.redacted_preview
                        })
                    })
                    .collect::<Vec<_>>();
                span.pii_entities.extend(new_entities);
            } else {
                spans.push(MutableTextSpan {
                    canonical_text: block.text.clone(),
                    normalized_text: normalized,
                    first_seen_at: scene.timestamp,
                    last_seen_at: scene.timestamp,
                    scene_ids: vec![scene.scene_id.clone()],
                    frontmost_app_name: scene.frontmost_app_name.clone(),
                    frontmost_bundle_id: scene.frontmost_bundle_id.clone(),
                    window_title: scene.window_title.clone(),
                    confidence_sum: block.confidence,
                    confidence_count: 1,
                    bbox_union: block.bbox.clone(),
                    occurrence_count: 1,
                    was_partial_match: false,
                    pii_entities: detect_pii_entities(&block.text, Some(block.bbox.clone())),
                    raw_row_ids: vec![block.block_id.clone()],
                });
            }
        }
    }

    spans
        .into_iter()
        .enumerate()
        .map(|(index, span)| AgentTextSpanDto {
            text_span_id: format!("span-{}-{}", session_id, index + 1),
            session_id: session_id.to_string(),
            canonical_text: span.canonical_text,
            normalized_text: span.normalized_text,
            first_seen_at: span.first_seen_at,
            last_seen_at: span.last_seen_at,
            duration_ms: (span.last_seen_at - span.first_seen_at).max(0),
            scene_ids: unique_strings(span.scene_ids),
            frontmost_app_name: span.frontmost_app_name,
            frontmost_bundle_id: span.frontmost_bundle_id,
            window_title: span.window_title,
            avg_confidence: span.confidence_sum / span.confidence_count as f32,
            bbox_union: span.bbox_union,
            occurrence_count: span.occurrence_count,
            was_partial_match: span.was_partial_match,
            pii_entities: span.pii_entities,
            raw_source: serde_json::json!({ "raw_row_ids": unique_strings(span.raw_row_ids) }),
        })
        .collect()
}

pub(super) async fn insert_text_span(
    db: &Arc<Database>,
    span: &AgentTextSpanDto,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        r#"
        INSERT INTO ocr_text_spans (
            text_span_id, session_id, canonical_text, normalized_text, first_seen_at, last_seen_at,
            duration_ms, scene_ids_json, frontmost_app_name, frontmost_bundle_id, window_title,
            avg_confidence, bbox_union_json, occurrence_count, was_partial_match, pii_entities_json,
            raw_source_json, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&span.text_span_id)
    .bind(&span.session_id)
    .bind(&span.canonical_text)
    .bind(&span.normalized_text)
    .bind(span.first_seen_at)
    .bind(span.last_seen_at)
    .bind(span.duration_ms)
    .bind(serde_json::to_string(&span.scene_ids)?)
    .bind(&span.frontmost_app_name)
    .bind(&span.frontmost_bundle_id)
    .bind(&span.window_title)
    .bind(span.avg_confidence as f64)
    .bind(serde_json::to_string(&span.bbox_union)?)
    .bind(span.occurrence_count as i64)
    .bind(if span.was_partial_match { 1 } else { 0 })
    .bind(serde_json::to_string(&span.pii_entities)?)
    .bind(serde_json::to_string(&span.raw_source)?)
    .bind(now)
    .bind(now)
    .execute(db.pool())
    .await?;
    Ok(())
}
