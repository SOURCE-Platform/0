use super::pii::{
    classify_entity_type, dominant_terms_from_texts, extract_title_hint, preview_text,
};
use super::{AgentContextEntityDto, AgentSceneSnapshotDto, AgentTextSpanDto, ENTITY_MAX_GAP_MS};
use crate::core::database::Database;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub(super) fn build_context_entities(
    session_id: &str,
    scenes: &[AgentSceneSnapshotDto],
    spans: &[AgentTextSpanDto],
) -> Vec<AgentContextEntityDto> {
    if scenes.is_empty() {
        return Vec::new();
    }

    let mut scene_groups: Vec<Vec<&AgentSceneSnapshotDto>> = Vec::new();
    let mut current_group: Vec<&AgentSceneSnapshotDto> = Vec::new();
    for scene in scenes {
        let should_split = current_group.last().map(|previous| {
            scene.frontmost_app_name != previous.frontmost_app_name
                || scene.frontmost_bundle_id != previous.frontmost_bundle_id
                || scene.window_title != previous.window_title
                || scene.timestamp - previous.timestamp > ENTITY_MAX_GAP_MS
        });
        if should_split.unwrap_or(false) && !current_group.is_empty() {
            scene_groups.push(current_group);
            current_group = Vec::new();
        }
        current_group.push(scene);
    }
    if !current_group.is_empty() {
        scene_groups.push(current_group);
    }

    scene_groups.into_iter().enumerate().map(|(index, group)| {
        let first = group.first().unwrap();
        let last = group.last().unwrap();
        let scene_ids = group.iter().map(|scene| scene.scene_id.clone()).collect::<Vec<_>>();
        let group_scene_set = scene_ids.iter().cloned().collect::<HashSet<_>>();
        let matched_spans = spans
            .iter()
            .filter(|span| span.scene_ids.iter().any(|scene_id| group_scene_set.contains(scene_id)))
            .cloned()
            .collect::<Vec<_>>();
        let summary_text = matched_spans.iter().map(|span| span.canonical_text.clone()).take(12).collect::<Vec<_>>().join(" ");
        let title_hint = first.window_title.clone().or_else(|| extract_title_hint(&first.full_text));
        let dominant_terms = dominant_terms_from_texts(
            &matched_spans.iter().map(|span| span.canonical_text.clone()).collect::<Vec<_>>(),
        );

        let mut pii_counts = HashMap::new();
        for span in &matched_spans {
            for entity in &span.pii_entities {
                *pii_counts.entry(entity.entity_type.clone()).or_insert(0) += 1;
            }
        }

        let confidence = if matched_spans.is_empty() {
            first.avg_confidence
        } else {
            matched_spans.iter().map(|span| span.avg_confidence).sum::<f32>() / matched_spans.len() as f32
        };

        AgentContextEntityDto {
            entity_id: format!("entity-{}-{}", session_id, index + 1),
            session_id: session_id.to_string(),
            entity_type: classify_entity_type(first.frontmost_app_name.as_deref(), first.window_title.as_deref()).to_string(),
            frontmost_app_name: first.frontmost_app_name.clone(),
            frontmost_bundle_id: first.frontmost_bundle_id.clone(),
            window_title: first.window_title.clone(),
            first_seen_at: first.timestamp,
            last_seen_at: last.timestamp,
            duration_ms: (last.timestamp - first.timestamp).max(0),
            scene_ids: scene_ids.clone(),
            text_span_ids: matched_spans.iter().map(|span| span.text_span_id.clone()).collect(),
            title_hint,
            summary_text: preview_text(&summary_text),
            dominant_terms,
            pii_entity_counts: pii_counts,
            confidence,
            raw_source: serde_json::json!({
                "scene_ids": scene_ids,
                "text_span_ids": matched_spans.iter().map(|span| span.text_span_id.clone()).collect::<Vec<_>>(),
            }),
        }
    }).collect()
}

pub(super) async fn insert_context_entity(
    db: &Arc<Database>,
    entity: &AgentContextEntityDto,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        r#"
        INSERT INTO ocr_context_entities (
            entity_id, session_id, entity_type, frontmost_app_name, frontmost_bundle_id,
            window_title, first_seen_at, last_seen_at, duration_ms, scene_ids_json,
            text_span_ids_json, title_hint, summary_text, dominant_terms_json,
            pii_entity_counts_json, raw_source_json, confidence, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&entity.entity_id)
    .bind(&entity.session_id)
    .bind(&entity.entity_type)
    .bind(&entity.frontmost_app_name)
    .bind(&entity.frontmost_bundle_id)
    .bind(&entity.window_title)
    .bind(entity.first_seen_at)
    .bind(entity.last_seen_at)
    .bind(entity.duration_ms)
    .bind(serde_json::to_string(&entity.scene_ids)?)
    .bind(serde_json::to_string(&entity.text_span_ids)?)
    .bind(&entity.title_hint)
    .bind(&entity.summary_text)
    .bind(serde_json::to_string(&entity.dominant_terms)?)
    .bind(serde_json::to_string(&entity.pii_entity_counts)?)
    .bind(serde_json::to_string(&entity.raw_source)?)
    .bind(entity.confidence as f64)
    .bind(now)
    .bind(now)
    .execute(db.pool())
    .await?;
    Ok(())
}
