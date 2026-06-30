use crate::core::database::Database;

use super::mappings::{entity_row_to_dto, scene_row_to_dto, span_row_to_dto};
use super::pii::{preview_text, snippet_for_query};
use super::{
    AgentContextEntityDto, AgentContextSearchResultDto, AgentSceneSnapshotDto, AgentTextSpanDto,
    ContextEntityRow, OcrAgentSummaryDto, SceneSnapshotRow, TextSpanRow,
};
use std::collections::HashMap;
use std::sync::Arc;

pub async fn get_scene_snapshot(
    db: &Arc<Database>,
    scene_id: &str,
) -> Result<Option<AgentSceneSnapshotDto>, Box<dyn std::error::Error + Send + Sync>> {
    sqlx::query_as::<_, SceneSnapshotRow>(
        r#"
        SELECT scene_id, session_id, timestamp, display_id, frontmost_app_name, frontmost_bundle_id,
               window_title, trigger_reason, frame_path, frame_width, frame_height, full_text,
               avg_confidence, block_count, text_blocks_json, pii_entities_json, linked_context_json,
               raw_source_json
        FROM ocr_scene_snapshots
        WHERE scene_id = ?
        LIMIT 1
        "#,
    )
    .bind(scene_id)
    .fetch_optional(db.pool())
    .await?
    .map(scene_row_to_dto)
    .transpose()
}

pub async fn get_scene_snapshots(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
) -> Result<Vec<AgentSceneSnapshotDto>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, SceneSnapshotRow>(
        r#"
        SELECT scene_id, session_id, timestamp, display_id, frontmost_app_name, frontmost_bundle_id,
               window_title, trigger_reason, frame_path, frame_width, frame_height, full_text,
               avg_confidence, block_count, text_blocks_json, pii_entities_json, linked_context_json,
               raw_source_json
        FROM ocr_scene_snapshots
        WHERE timestamp >= ? AND timestamp <= ?
        ORDER BY timestamp ASC
        "#,
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await?;

    filter_by_app(rows.into_iter().map(scene_row_to_dto), app_filter, |dto| {
        dto.frontmost_app_name.as_deref()
    })
}

pub async fn get_text_spans(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
) -> Result<Vec<AgentTextSpanDto>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, TextSpanRow>(
        r#"
        SELECT text_span_id, session_id, canonical_text, normalized_text, first_seen_at, last_seen_at,
               duration_ms, scene_ids_json, frontmost_app_name, frontmost_bundle_id, window_title,
               avg_confidence, bbox_union_json, occurrence_count, was_partial_match,
               pii_entities_json, raw_source_json
        FROM ocr_text_spans
        WHERE last_seen_at >= ? AND first_seen_at <= ?
        ORDER BY duration_ms DESC, first_seen_at ASC
        "#,
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await?;

    filter_by_app(rows.into_iter().map(span_row_to_dto), app_filter, |dto| {
        dto.frontmost_app_name.as_deref()
    })
}

pub async fn get_context_entities(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
    entity_type: Option<String>,
) -> Result<Vec<AgentContextEntityDto>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, ContextEntityRow>(
        r#"
        SELECT entity_id, session_id, entity_type, frontmost_app_name, frontmost_bundle_id,
               window_title, first_seen_at, last_seen_at, duration_ms, scene_ids_json,
               text_span_ids_json, title_hint, summary_text, dominant_terms_json,
               pii_entity_counts_json, raw_source_json, confidence
        FROM ocr_context_entities
        WHERE last_seen_at >= ? AND first_seen_at <= ?
        ORDER BY duration_ms DESC, first_seen_at DESC
        "#,
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await?;

    let mut entities = Vec::new();
    for row in rows {
        let dto = entity_row_to_dto(row)?;
        if app_filter
            .as_ref()
            .map(|filter| dto.frontmost_app_name.as_deref() == Some(filter.as_str()))
            .unwrap_or(true)
            && entity_type
                .as_ref()
                .map(|filter| dto.entity_type == *filter)
                .unwrap_or(true)
        {
            entities.push(dto);
        }
    }
    Ok(entities)
}

pub async fn search_agent_context(
    db: &Arc<Database>,
    query: &str,
    start_timestamp: Option<i64>,
    end_timestamp: Option<i64>,
    app_filter: Option<String>,
) -> Result<Vec<AgentContextSearchResultDto>, Box<dyn std::error::Error + Send + Sync>> {
    let needle = query.to_lowercase();
    let start = start_timestamp.unwrap_or(i64::MIN / 2);
    let end = end_timestamp.unwrap_or(i64::MAX / 2);
    let mut results = Vec::new();

    for scene in get_scene_snapshots(db, start, end, app_filter.clone()).await? {
        if scene.full_text.to_lowercase().contains(&needle) {
            results.push(AgentContextSearchResultDto {
                match_kind: "scene_snapshot".to_string(),
                scene_id: Some(scene.scene_id.clone()),
                text_span_id: None,
                entity_id: None,
                session_id: scene.session_id.clone(),
                timestamp: scene.timestamp,
                app_name: scene.frontmost_app_name.clone(),
                title: scene
                    .frontmost_app_name
                    .clone()
                    .unwrap_or_else(|| "Scene snapshot".to_string()),
                snippet: snippet_for_query(&scene.full_text, query, 120),
                confidence: scene.avg_confidence,
                raw_source: serde_json::to_value(&scene.raw_source)?,
            });
        }
    }

    for span in get_text_spans(db, start, end, app_filter.clone()).await? {
        if span.canonical_text.to_lowercase().contains(&needle) {
            results.push(AgentContextSearchResultDto {
                match_kind: "text_span".to_string(),
                scene_id: span.scene_ids.first().cloned(),
                text_span_id: Some(span.text_span_id.clone()),
                entity_id: None,
                session_id: span.session_id.clone(),
                timestamp: span.first_seen_at,
                app_name: span.frontmost_app_name.clone(),
                title: preview_text(&span.canonical_text),
                snippet: snippet_for_query(&span.canonical_text, query, 120),
                confidence: span.avg_confidence,
                raw_source: span.raw_source.clone(),
            });
        }
    }

    for entity in get_context_entities(db, start, end, app_filter, None).await? {
        let haystack = format!(
            "{} {} {}",
            entity.title_hint.clone().unwrap_or_default(),
            entity.summary_text,
            entity.dominant_terms.join(" ")
        )
        .to_lowercase();
        if haystack.contains(&needle) {
            results.push(AgentContextSearchResultDto {
                match_kind: "context_entity".to_string(),
                scene_id: entity.scene_ids.first().cloned(),
                text_span_id: entity.text_span_ids.first().cloned(),
                entity_id: Some(entity.entity_id.clone()),
                session_id: entity.session_id.clone(),
                timestamp: entity.first_seen_at,
                app_name: entity.frontmost_app_name.clone(),
                title: entity
                    .title_hint
                    .clone()
                    .unwrap_or_else(|| entity.entity_type.clone()),
                snippet: snippet_for_query(&entity.summary_text, query, 120),
                confidence: entity.confidence,
                raw_source: entity.raw_source.clone(),
            });
        }
    }

    results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(results)
}

pub async fn get_activity_episode(
    db: &Arc<Database>,
    timestamp: i64,
) -> Result<super::ActivityEpisodeDto, Box<dyn std::error::Error + Send + Sync>> {
    let scene = sqlx::query_as::<_, SceneSnapshotRow>(
        r#"
        SELECT scene_id, session_id, timestamp, display_id, frontmost_app_name, frontmost_bundle_id,
               window_title, trigger_reason, frame_path, frame_width, frame_height, full_text,
               avg_confidence, block_count, text_blocks_json, pii_entities_json, linked_context_json,
               raw_source_json
        FROM ocr_scene_snapshots
        ORDER BY ABS(timestamp - ?) ASC
        LIMIT 1
        "#,
    )
    .bind(timestamp)
    .fetch_optional(db.pool())
    .await?
    .map(scene_row_to_dto)
    .transpose()?;

    let context_entity = sqlx::query_as::<_, ContextEntityRow>(
        r#"
        SELECT entity_id, session_id, entity_type, frontmost_app_name, frontmost_bundle_id,
               window_title, first_seen_at, last_seen_at, duration_ms, scene_ids_json,
               text_span_ids_json, title_hint, summary_text, dominant_terms_json,
               pii_entity_counts_json, raw_source_json, confidence
        FROM ocr_context_entities
        WHERE first_seen_at <= ? AND last_seen_at >= ?
        ORDER BY duration_ms DESC
        LIMIT 1
        "#,
    )
    .bind(timestamp)
    .bind(timestamp)
    .fetch_optional(db.pool())
    .await?
    .map(entity_row_to_dto)
    .transpose()?;

    let text_spans = sqlx::query_as::<_, TextSpanRow>(
        r#"
        SELECT text_span_id, session_id, canonical_text, normalized_text, first_seen_at, last_seen_at,
               duration_ms, scene_ids_json, frontmost_app_name, frontmost_bundle_id, window_title,
               avg_confidence, bbox_union_json, occurrence_count, was_partial_match,
               pii_entities_json, raw_source_json
        FROM ocr_text_spans
        WHERE first_seen_at <= ? AND last_seen_at >= ?
        ORDER BY duration_ms DESC
        LIMIT 20
        "#,
    )
    .bind(timestamp)
    .bind(timestamp)
    .fetch_all(db.pool())
    .await?
    .into_iter()
    .map(span_row_to_dto)
    .collect::<Result<Vec<_>, _>>()?;

    Ok(super::ActivityEpisodeDto {
        timestamp,
        scene,
        text_spans,
        context_entity,
    })
}

pub async fn get_ocr_agent_summary(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<OcrAgentSummaryDto, Box<dyn std::error::Error + Send + Sync>> {
    let scenes = get_scene_snapshots(db, start_timestamp, end_timestamp, None).await?;
    let spans = get_text_spans(db, start_timestamp, end_timestamp, None).await?;
    let entities = get_context_entities(db, start_timestamp, end_timestamp, None, None).await?;

    let mut app_counts = HashMap::new();
    let mut term_counts = HashMap::new();
    let mut total_visible_text_duration_ms = 0_i64;
    for span in &spans {
        total_visible_text_duration_ms += span.duration_ms.max(0);
        if let Some(app) = span.frontmost_app_name.clone() {
            *app_counts.entry(app).or_insert(0) += 1;
        }
    }
    for entity in &entities {
        for term in &entity.dominant_terms {
            *term_counts.entry(term.clone()).or_insert(0) += 1;
        }
    }

    let mut top_apps = app_counts.into_iter().collect::<Vec<_>>();
    top_apps.sort_by(|a, b| b.1.cmp(&a.1));
    let mut dominant_terms = term_counts.into_iter().collect::<Vec<_>>();
    dominant_terms.sort_by(|a, b| b.1.cmp(&a.1));

    Ok(OcrAgentSummaryDto {
        start_timestamp,
        end_timestamp,
        scene_count: scenes.len(),
        text_span_count: spans.len(),
        entity_count: entities.len(),
        total_visible_text_duration_ms,
        top_apps: top_apps.into_iter().take(8).map(|(app, _)| app).collect(),
        dominant_terms: dominant_terms
            .into_iter()
            .take(12)
            .map(|(term, _)| term)
            .collect(),
    })
}

fn filter_by_app<T, I, F>(
    values: I,
    app_filter: Option<String>,
    app_accessor: F,
) -> Result<Vec<T>, Box<dyn std::error::Error + Send + Sync>>
where
    I: IntoIterator<Item = Result<T, Box<dyn std::error::Error + Send + Sync>>>,
    F: Fn(&T) -> Option<&str>,
{
    let mut results = Vec::new();
    for value in values {
        let value = value?;
        if app_filter
            .as_ref()
            .map(|filter| app_accessor(&value) == Some(filter.as_str()))
            .unwrap_or(true)
        {
            results.push(value);
        }
    }
    Ok(results)
}
