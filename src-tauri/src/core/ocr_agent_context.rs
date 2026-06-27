use crate::core::database::Database;
use crate::core::ocr_storage::ProcessedOcrResult;
use crate::models::ocr::BoundingBox;
use image::GenericImageView;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

const SPAN_MAX_GAP_MS: i64 = 15_000;
const ENTITY_MAX_GAP_MS: i64 = 120_000;
const BBOX_POSITION_TOLERANCE: i64 = 80;
const APP_CONTEXT_LOOKBACK_MS: i64 = 60_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTextBlockDto {
    pub block_id: String,
    pub text: String,
    pub confidence: f32,
    pub bbox: BoundingBox,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPiiEntityDto {
    pub entity_type: String,
    pub redacted_preview: String,
    pub confidence: f32,
    pub context_text: String,
    pub bounding_box: Option<BoundingBox>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentLinkedContextDto {
    pub focused_app: Option<String>,
    pub focused_bundle_id: Option<String>,
    pub window_title: Option<String>,
    pub visible_window_snapshot_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRawSourceDto {
    pub raw_row_ids: Vec<String>,
    pub frame_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSceneSnapshotDto {
    pub scene_id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub display_id: Option<u32>,
    pub frontmost_app_name: Option<String>,
    pub frontmost_bundle_id: Option<String>,
    pub window_title: Option<String>,
    pub trigger_reason: String,
    pub frame_path: Option<String>,
    pub frame_width: Option<u32>,
    pub frame_height: Option<u32>,
    pub full_text: String,
    pub avg_confidence: f32,
    pub block_count: usize,
    pub text_blocks: Vec<AgentTextBlockDto>,
    pub pii_entities: Vec<AgentPiiEntityDto>,
    pub linked_context: AgentLinkedContextDto,
    pub raw_source: AgentRawSourceDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTextSpanDto {
    pub text_span_id: String,
    pub session_id: String,
    pub canonical_text: String,
    pub normalized_text: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub duration_ms: i64,
    pub scene_ids: Vec<String>,
    pub frontmost_app_name: Option<String>,
    pub frontmost_bundle_id: Option<String>,
    pub window_title: Option<String>,
    pub avg_confidence: f32,
    pub bbox_union: BoundingBox,
    pub occurrence_count: usize,
    pub was_partial_match: bool,
    pub pii_entities: Vec<AgentPiiEntityDto>,
    pub raw_source: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentContextEntityDto {
    pub entity_id: String,
    pub session_id: String,
    pub entity_type: String,
    pub frontmost_app_name: Option<String>,
    pub frontmost_bundle_id: Option<String>,
    pub window_title: Option<String>,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub duration_ms: i64,
    pub scene_ids: Vec<String>,
    pub text_span_ids: Vec<String>,
    pub title_hint: Option<String>,
    pub summary_text: String,
    pub dominant_terms: Vec<String>,
    pub pii_entity_counts: HashMap<String, usize>,
    pub confidence: f32,
    pub raw_source: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentContextSearchResultDto {
    pub match_kind: String,
    pub scene_id: Option<String>,
    pub text_span_id: Option<String>,
    pub entity_id: Option<String>,
    pub session_id: String,
    pub timestamp: i64,
    pub app_name: Option<String>,
    pub title: String,
    pub snippet: String,
    pub confidence: f32,
    pub raw_source: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEpisodeDto {
    pub timestamp: i64,
    pub scene: Option<AgentSceneSnapshotDto>,
    pub text_spans: Vec<AgentTextSpanDto>,
    pub context_entity: Option<AgentContextEntityDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrAgentSummaryDto {
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub scene_count: usize,
    pub text_span_count: usize,
    pub entity_count: usize,
    pub total_visible_text_duration_ms: i64,
    pub top_apps: Vec<String>,
    pub dominant_terms: Vec<String>,
}

#[derive(Debug, Clone, FromRow)]
struct WindowSnapshotContextRow {
    id: String,
    timestamp: i64,
    frontmost_app_name: Option<String>,
    frontmost_bundle_id: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct KeyboardContextRow {
    timestamp: i64,
    app_name: String,
    window_title: String,
}

#[derive(Debug, Clone, FromRow)]
struct AppUsageContextRow {
    app_name: String,
    bundle_id: String,
}

#[derive(Debug, Clone, FromRow)]
struct SceneSnapshotRow {
    scene_id: String,
    session_id: String,
    timestamp: i64,
    display_id: Option<i64>,
    frontmost_app_name: Option<String>,
    frontmost_bundle_id: Option<String>,
    window_title: Option<String>,
    trigger_reason: String,
    frame_path: Option<String>,
    frame_width: Option<i64>,
    frame_height: Option<i64>,
    full_text: String,
    avg_confidence: f64,
    block_count: i64,
    text_blocks_json: String,
    pii_entities_json: String,
    linked_context_json: String,
    raw_source_json: String,
}

#[derive(Debug, Clone, FromRow)]
struct TextSpanRow {
    text_span_id: String,
    session_id: String,
    canonical_text: String,
    normalized_text: String,
    first_seen_at: i64,
    last_seen_at: i64,
    duration_ms: i64,
    scene_ids_json: String,
    frontmost_app_name: Option<String>,
    frontmost_bundle_id: Option<String>,
    window_title: Option<String>,
    avg_confidence: f64,
    bbox_union_json: String,
    occurrence_count: i64,
    was_partial_match: i64,
    pii_entities_json: String,
    raw_source_json: String,
}

#[derive(Debug, Clone, FromRow)]
struct ContextEntityRow {
    entity_id: String,
    session_id: String,
    entity_type: String,
    frontmost_app_name: Option<String>,
    frontmost_bundle_id: Option<String>,
    window_title: Option<String>,
    first_seen_at: i64,
    last_seen_at: i64,
    duration_ms: i64,
    scene_ids_json: String,
    text_span_ids_json: String,
    title_hint: Option<String>,
    summary_text: String,
    dominant_terms_json: String,
    pii_entity_counts_json: String,
    raw_source_json: String,
    confidence: f64,
}

#[derive(Debug, Clone, Default)]
struct InferredAppContext {
    app_name: Option<String>,
    bundle_id: Option<String>,
    window_title: Option<String>,
    visible_window_snapshot_id: Option<String>,
}

#[derive(Debug, Clone)]
struct MutableTextSpan {
    canonical_text: String,
    normalized_text: String,
    first_seen_at: i64,
    last_seen_at: i64,
    scene_ids: Vec<String>,
    frontmost_app_name: Option<String>,
    frontmost_bundle_id: Option<String>,
    window_title: Option<String>,
    confidence_sum: f32,
    confidence_count: usize,
    bbox_union: BoundingBox,
    occurrence_count: usize,
    was_partial_match: bool,
    pii_entities: Vec<AgentPiiEntityDto>,
    raw_row_ids: Vec<String>,
}

pub async fn reindex_processed_result(
    db: &Arc<Database>,
    result: &ProcessedOcrResult,
    raw_row_ids: &[String],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let scene = build_scene_snapshot(db, result, raw_row_ids).await?;
    upsert_scene_snapshot(db, &scene).await?;
    rebuild_session_spans_and_entities(db, &result.session_id.to_string()).await?;
    Ok(())
}

pub async fn delete_derived_for_session(
    db: &Arc<Database>,
    session_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    sqlx::query("DELETE FROM ocr_context_entities WHERE session_id = ?")
        .bind(session_id)
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM ocr_text_spans WHERE session_id = ?")
        .bind(session_id)
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM ocr_scene_snapshots WHERE session_id = ?")
        .bind(session_id)
        .execute(db.pool())
        .await?;
    Ok(())
}

pub async fn delete_all_derived(
    db: &Arc<Database>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    sqlx::query("DELETE FROM ocr_context_entities")
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM ocr_text_spans")
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM ocr_scene_snapshots")
        .execute(db.pool())
        .await?;
    Ok(())
}

pub async fn get_scene_snapshot(
    db: &Arc<Database>,
    scene_id: &str,
) -> Result<Option<AgentSceneSnapshotDto>, Box<dyn std::error::Error + Send + Sync>> {
    let row = sqlx::query_as::<_, SceneSnapshotRow>(
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
    .await?;

    row.map(scene_row_to_dto).transpose()
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

    let mut snapshots = Vec::new();
    for row in rows {
        let dto = scene_row_to_dto(row)?;
        if app_filter
            .as_ref()
            .map(|filter| dto.frontmost_app_name.as_deref() == Some(filter.as_str()))
            .unwrap_or(true)
        {
            snapshots.push(dto);
        }
    }

    Ok(snapshots)
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

    let mut spans = Vec::new();
    for row in rows {
        let dto = span_row_to_dto(row)?;
        if app_filter
            .as_ref()
            .map(|filter| dto.frontmost_app_name.as_deref() == Some(filter.as_str()))
            .unwrap_or(true)
        {
            spans.push(dto);
        }
    }

    Ok(spans)
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
) -> Result<ActivityEpisodeDto, Box<dyn std::error::Error + Send + Sync>> {
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

    Ok(ActivityEpisodeDto {
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

    let mut app_counts: HashMap<String, usize> = HashMap::new();
    let mut term_counts: HashMap<String, usize> = HashMap::new();
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

async fn build_scene_snapshot(
    db: &Arc<Database>,
    result: &ProcessedOcrResult,
    raw_row_ids: &[String],
) -> Result<AgentSceneSnapshotDto, Box<dyn std::error::Error + Send + Sync>> {
    let session_id = result.session_id.to_string();
    let scene_id = format!("scene-{}-{}", session_id, result.timestamp);
    let context = infer_app_context(db, result.timestamp, Some(&session_id)).await?;

    let frame_path_string = result
        .frame_path
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());

    let text_blocks = result
        .ocr_result
        .text_blocks
        .iter()
        .enumerate()
        .map(|(index, block)| AgentTextBlockDto {
            block_id: raw_row_ids
                .get(index)
                .cloned()
                .unwrap_or_else(|| format!("{}-block-{}", scene_id, index + 1)),
            text: block.text.clone(),
            confidence: block.confidence,
            bbox: block.bounding_box.clone(),
            language: block.language.clone(),
        })
        .collect::<Vec<_>>();

    let avg_confidence = if text_blocks.is_empty() {
        0.0
    } else {
        text_blocks
            .iter()
            .map(|block| block.confidence)
            .sum::<f32>()
            / text_blocks.len() as f32
    };

    let pii_entities = text_blocks
        .iter()
        .flat_map(|block| detect_pii_entities(&block.text, Some(block.bbox.clone())))
        .collect::<Vec<_>>();

    Ok(AgentSceneSnapshotDto {
        scene_id,
        session_id: session_id.clone(),
        timestamp: result.timestamp,
        display_id: result.display_id,
        frontmost_app_name: context.app_name.clone(),
        frontmost_bundle_id: context.bundle_id.clone(),
        window_title: context.window_title.clone(),
        trigger_reason: result.trigger_reason.clone(),
        frame_path: frame_path_string.clone(),
        frame_width: Some(result.frame_width),
        frame_height: Some(result.frame_height),
        full_text: result.ocr_result.total_text.clone(),
        avg_confidence,
        block_count: text_blocks.len(),
        text_blocks,
        pii_entities,
        linked_context: AgentLinkedContextDto {
            focused_app: context.app_name,
            focused_bundle_id: context.bundle_id,
            window_title: context.window_title,
            visible_window_snapshot_id: context.visible_window_snapshot_id,
        },
        raw_source: AgentRawSourceDto {
            raw_row_ids: raw_row_ids.to_vec(),
            frame_path: frame_path_string,
        },
    })
}

async fn upsert_scene_snapshot(
    db: &Arc<Database>,
    scene: &AgentSceneSnapshotDto,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        r#"
        INSERT INTO ocr_scene_snapshots (
            scene_id, session_id, timestamp, display_id, frontmost_app_name, frontmost_bundle_id,
            window_title, trigger_reason, frame_path, frame_width, frame_height, full_text,
            avg_confidence, block_count, text_blocks_json, pii_entities_json, linked_context_json,
            raw_source_json, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(scene_id) DO UPDATE SET
            session_id = excluded.session_id,
            timestamp = excluded.timestamp,
            display_id = excluded.display_id,
            frontmost_app_name = excluded.frontmost_app_name,
            frontmost_bundle_id = excluded.frontmost_bundle_id,
            window_title = excluded.window_title,
            trigger_reason = excluded.trigger_reason,
            frame_path = excluded.frame_path,
            frame_width = excluded.frame_width,
            frame_height = excluded.frame_height,
            full_text = excluded.full_text,
            avg_confidence = excluded.avg_confidence,
            block_count = excluded.block_count,
            text_blocks_json = excluded.text_blocks_json,
            pii_entities_json = excluded.pii_entities_json,
            linked_context_json = excluded.linked_context_json,
            raw_source_json = excluded.raw_source_json,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&scene.scene_id)
    .bind(&scene.session_id)
    .bind(scene.timestamp)
    .bind(scene.display_id.map(|value| value as i64))
    .bind(&scene.frontmost_app_name)
    .bind(&scene.frontmost_bundle_id)
    .bind(&scene.window_title)
    .bind(&scene.trigger_reason)
    .bind(&scene.frame_path)
    .bind(scene.frame_width.map(|value| value as i64))
    .bind(scene.frame_height.map(|value| value as i64))
    .bind(&scene.full_text)
    .bind(scene.avg_confidence as f64)
    .bind(scene.block_count as i64)
    .bind(serde_json::to_string(&scene.text_blocks)?)
    .bind(serde_json::to_string(&scene.pii_entities)?)
    .bind(serde_json::to_string(&scene.linked_context)?)
    .bind(serde_json::to_string(&scene.raw_source)?)
    .bind(now)
    .bind(now)
    .execute(db.pool())
    .await?;

    Ok(())
}

async fn rebuild_session_spans_and_entities(
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

    let entities = build_context_entities(session_id, &scenes, &spans);
    for entity in &entities {
        insert_context_entity(db, entity).await?;
    }

    Ok(())
}

fn build_text_spans(session_id: &str, scenes: &[AgentSceneSnapshotDto]) -> Vec<AgentTextSpanDto> {
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
            raw_source: serde_json::json!({
                "raw_row_ids": unique_strings(span.raw_row_ids),
            }),
        })
        .collect()
}

fn build_context_entities(
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

    scene_groups
        .into_iter()
        .enumerate()
        .map(|(index, group)| {
            let first = group.first().unwrap();
            let last = group.last().unwrap();
            let scene_ids = group.iter().map(|scene| scene.scene_id.clone()).collect::<Vec<_>>();
            let group_scene_set = scene_ids.iter().cloned().collect::<HashSet<_>>();
            let matched_spans = spans
                .iter()
                .filter(|span| span.scene_ids.iter().any(|scene_id| group_scene_set.contains(scene_id)))
                .cloned()
                .collect::<Vec<_>>();

            let summary_text = matched_spans
                .iter()
                .map(|span| span.canonical_text.clone())
                .take(12)
                .collect::<Vec<_>>()
                .join(" ");
            let title_hint = first
                .window_title
                .clone()
                .or_else(|| extract_title_hint(&first.full_text));
            let dominant_terms = dominant_terms_from_texts(
                &matched_spans
                    .iter()
                    .map(|span| span.canonical_text.clone())
                    .collect::<Vec<_>>(),
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
                matched_spans.iter().map(|span| span.avg_confidence).sum::<f32>()
                    / matched_spans.len() as f32
            };

            AgentContextEntityDto {
                entity_id: format!("entity-{}-{}", session_id, index + 1),
                session_id: session_id.to_string(),
                entity_type: classify_entity_type(
                    first.frontmost_app_name.as_deref(),
                    first.window_title.as_deref(),
                )
                .to_string(),
                frontmost_app_name: first.frontmost_app_name.clone(),
                frontmost_bundle_id: first.frontmost_bundle_id.clone(),
                window_title: first.window_title.clone(),
                first_seen_at: first.timestamp,
                last_seen_at: last.timestamp,
                duration_ms: (last.timestamp - first.timestamp).max(0),
                scene_ids: scene_ids.clone(),
                text_span_ids: matched_spans
                    .iter()
                    .map(|span| span.text_span_id.clone())
                    .collect(),
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
        })
        .collect()
}

async fn insert_text_span(
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

async fn insert_context_entity(
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

fn scene_row_to_dto(
    row: SceneSnapshotRow,
) -> Result<AgentSceneSnapshotDto, Box<dyn std::error::Error + Send + Sync>> {
    Ok(AgentSceneSnapshotDto {
        scene_id: row.scene_id,
        session_id: row.session_id,
        timestamp: row.timestamp,
        display_id: row.display_id.map(|value| value as u32),
        frontmost_app_name: row.frontmost_app_name,
        frontmost_bundle_id: row.frontmost_bundle_id,
        window_title: row.window_title,
        trigger_reason: row.trigger_reason,
        frame_path: row.frame_path,
        frame_width: row.frame_width.map(|value| value as u32),
        frame_height: row.frame_height.map(|value| value as u32),
        full_text: row.full_text,
        avg_confidence: row.avg_confidence as f32,
        block_count: row.block_count as usize,
        text_blocks: serde_json::from_str(&row.text_blocks_json)?,
        pii_entities: serde_json::from_str(&row.pii_entities_json)?,
        linked_context: serde_json::from_str(&row.linked_context_json)?,
        raw_source: serde_json::from_str(&row.raw_source_json)?,
    })
}

fn span_row_to_dto(
    row: TextSpanRow,
) -> Result<AgentTextSpanDto, Box<dyn std::error::Error + Send + Sync>> {
    Ok(AgentTextSpanDto {
        text_span_id: row.text_span_id,
        session_id: row.session_id,
        canonical_text: row.canonical_text,
        normalized_text: row.normalized_text,
        first_seen_at: row.first_seen_at,
        last_seen_at: row.last_seen_at,
        duration_ms: row.duration_ms,
        scene_ids: serde_json::from_str(&row.scene_ids_json)?,
        frontmost_app_name: row.frontmost_app_name,
        frontmost_bundle_id: row.frontmost_bundle_id,
        window_title: row.window_title,
        avg_confidence: row.avg_confidence as f32,
        bbox_union: serde_json::from_str(&row.bbox_union_json)?,
        occurrence_count: row.occurrence_count as usize,
        was_partial_match: row.was_partial_match != 0,
        pii_entities: serde_json::from_str(&row.pii_entities_json)?,
        raw_source: serde_json::from_str(&row.raw_source_json)?,
    })
}

fn entity_row_to_dto(
    row: ContextEntityRow,
) -> Result<AgentContextEntityDto, Box<dyn std::error::Error + Send + Sync>> {
    Ok(AgentContextEntityDto {
        entity_id: row.entity_id,
        session_id: row.session_id,
        entity_type: row.entity_type,
        frontmost_app_name: row.frontmost_app_name,
        frontmost_bundle_id: row.frontmost_bundle_id,
        window_title: row.window_title,
        first_seen_at: row.first_seen_at,
        last_seen_at: row.last_seen_at,
        duration_ms: row.duration_ms,
        scene_ids: serde_json::from_str(&row.scene_ids_json)?,
        text_span_ids: serde_json::from_str(&row.text_span_ids_json)?,
        title_hint: row.title_hint,
        summary_text: row.summary_text,
        dominant_terms: serde_json::from_str(&row.dominant_terms_json)?,
        pii_entity_counts: serde_json::from_str(&row.pii_entity_counts_json)?,
        raw_source: serde_json::from_str(&row.raw_source_json)?,
        confidence: row.confidence as f32,
    })
}

async fn infer_app_context(
    db: &Arc<Database>,
    timestamp: i64,
    session_id: Option<&str>,
) -> Result<InferredAppContext, Box<dyn std::error::Error + Send + Sync>> {
    let snapshot = sqlx::query_as::<_, WindowSnapshotContextRow>(
        r#"
        SELECT id, timestamp, frontmost_app_name, frontmost_bundle_id
        FROM window_snapshots
        WHERE timestamp <= ?
          AND timestamp >= ?
        ORDER BY timestamp DESC
        LIMIT 1
        "#,
    )
    .bind(timestamp)
    .bind(timestamp - APP_CONTEXT_LOOKBACK_MS)
    .fetch_optional(db.pool())
    .await?;

    if let Some(snapshot) = snapshot {
        return Ok(InferredAppContext {
            app_name: snapshot.frontmost_app_name,
            bundle_id: snapshot.frontmost_bundle_id,
            window_title: None,
            visible_window_snapshot_id: Some(snapshot.id),
        });
    }

    let keyboard_query = if session_id.is_some() {
        r#"
        SELECT timestamp, app_name, window_title
        FROM keyboard_events
        WHERE timestamp <= ?
          AND timestamp >= ?
          AND session_id = ?
        ORDER BY timestamp DESC
        LIMIT 1
        "#
    } else {
        r#"
        SELECT timestamp, app_name, window_title
        FROM keyboard_events
        WHERE timestamp <= ?
          AND timestamp >= ?
        ORDER BY timestamp DESC
        LIMIT 1
        "#
    };
    let mut query = sqlx::query_as::<_, KeyboardContextRow>(keyboard_query)
        .bind(timestamp)
        .bind(timestamp - APP_CONTEXT_LOOKBACK_MS);
    if let Some(session_id) = session_id {
        query = query.bind(session_id);
    }
    if let Some(event) = query.fetch_optional(db.pool()).await? {
        return Ok(InferredAppContext {
            app_name: Some(event.app_name),
            bundle_id: None,
            window_title: Some(event.window_title),
            visible_window_snapshot_id: None,
        });
    }

    let app_usage = sqlx::query_as::<_, AppUsageContextRow>(
        r#"
        SELECT app_name, bundle_id
        FROM app_usage
        WHERE start_timestamp <= ?
          AND (end_timestamp IS NULL OR end_timestamp >= ?)
        ORDER BY start_timestamp DESC
        LIMIT 1
        "#,
    )
    .bind(timestamp)
    .bind(timestamp)
    .fetch_optional(db.pool())
    .await?;

    Ok(InferredAppContext {
        app_name: app_usage.as_ref().map(|row| row.app_name.clone()),
        bundle_id: app_usage.as_ref().map(|row| row.bundle_id.clone()),
        window_title: None,
        visible_window_snapshot_id: None,
    })
}

fn detect_pii_entities(text: &str, bbox: Option<BoundingBox>) -> Vec<AgentPiiEntityDto> {
    detect_pii_spans(text)
        .into_iter()
        .map(|(entity_type, matched)| AgentPiiEntityDto {
            entity_type,
            redacted_preview: redact_match(&matched),
            confidence: 0.72,
            context_text: text.to_string(),
            bounding_box: bbox.clone(),
        })
        .collect()
}

fn detect_pii_spans(text: &str) -> Vec<(String, String)> {
    let patterns = vec![
        (
            "email",
            Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b").unwrap(),
        ),
        (
            "phone",
            Regex::new(r"\b(?:\+?\d{1,3}[-.\s]?)?(?:\(?\d{3}\)?[-.\s]?){1}\d{3}[-.\s]?\d{4}\b")
                .unwrap(),
        ),
        (
            "government_id",
            Regex::new(r"\b\d{3}[- ]?\d{2}[- ]?\d{4}\b").unwrap(),
        ),
        (
            "credit_card",
            Regex::new(r"\b(?:\d[ -]*?){13,19}\b").unwrap(),
        ),
    ];

    let mut matches = Vec::new();
    for (entity_type, regex) in patterns {
        for capture in regex.find_iter(text) {
            matches.push((entity_type.to_string(), capture.as_str().to_string()));
        }
    }
    matches
}

fn redact_match(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= 4 {
        "••••".to_string()
    } else {
        format!("{}••••", chars.iter().take(4).collect::<String>())
    }
}

fn normalize_text(value: &str) -> String {
    value
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn bbox_is_continuous(left: &BoundingBox, right: &BoundingBox) -> bool {
    if left.overlaps_with(right) {
        return true;
    }

    let dx = (left.x as i64 - right.x as i64).abs();
    let dy = (left.y as i64 - right.y as i64).abs();
    dx <= BBOX_POSITION_TOLERANCE && dy <= BBOX_POSITION_TOLERANCE
}

fn union_bbox(left: &BoundingBox, right: &BoundingBox) -> BoundingBox {
    let min_x = left.x.min(right.x);
    let min_y = left.y.min(right.y);
    let max_x = (left.x + left.width).max(right.x + right.width);
    let max_y = (left.y + left.height).max(right.y + right.height);
    BoundingBox::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

fn unique_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            unique.push(value);
        }
    }
    unique
}

fn preview_text(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= 96 {
        normalized
    } else {
        format!("{}...", normalized.chars().take(96).collect::<String>())
    }
}

fn snippet_for_query(text: &str, query: &str, max_length: usize) -> String {
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();
    if let Some(position) = text_lower.find(&query_lower) {
        let start = position.saturating_sub(max_length / 2);
        let end = (position + query.len() + max_length / 2).min(text.len());
        let mut snippet = text
            .chars()
            .skip(start)
            .take(end - start)
            .collect::<String>();
        if start > 0 {
            snippet = format!("...{}", snippet);
        }
        if end < text.len() {
            snippet = format!("{}...", snippet);
        }
        snippet
    } else {
        preview_text(text)
    }
}

fn classify_entity_type(app_name: Option<&str>, window_title: Option<&str>) -> &'static str {
    let app = app_name.unwrap_or_default().to_lowercase();
    let title = window_title.unwrap_or_default().to_lowercase();

    if app.contains("chrome")
        || app.contains("safari")
        || app.contains("firefox")
        || app.contains("arc")
        || title.contains("http")
    {
        "web_page"
    } else if app.contains("cursor")
        || app.contains("code")
        || app.contains("xcode")
        || app.contains("windsurf")
    {
        "code_editor_view"
    } else if app.contains("slack")
        || app.contains("discord")
        || app.contains("messages")
        || app.contains("telegram")
        || app.contains("whatsapp")
        || app.contains("claude")
        || app.contains("chatgpt")
        || app.contains("codex")
    {
        "chat_view"
    } else if app.contains("preview")
        || app.contains("word")
        || app.contains("pages")
        || app.contains("notes")
        || app.contains("pdf")
    {
        "document_view"
    } else {
        "unknown_view"
    }
}

fn extract_title_hint(text: &str) -> Option<String> {
    let first_line = text.lines().next().unwrap_or("").trim();
    if first_line.is_empty() {
        None
    } else {
        Some(preview_text(first_line))
    }
}

fn dominant_terms_from_texts(texts: &[String]) -> Vec<String> {
    let stop_words = [
        "the", "and", "for", "that", "with", "this", "from", "you", "your", "have", "not", "are",
        "was", "but", "they", "their", "into", "what", "when", "how", "why", "where", "http",
        "https", "www", "com",
    ];
    let stop_set = stop_words.into_iter().collect::<HashSet<_>>();
    let mut counts: HashMap<String, usize> = HashMap::new();

    for text in texts {
        for term in normalize_text(text)
            .split(|character: char| !character.is_alphanumeric())
            .filter(|term| term.len() >= 3)
        {
            if stop_set.contains(term) {
                continue;
            }
            *counts.entry(term.to_string()).or_insert(0) += 1;
        }
    }

    let mut terms = counts.into_iter().collect::<Vec<_>>();
    terms.sort_by(|a, b| b.1.cmp(&a.1));
    terms.into_iter().take(12).map(|(term, _)| term).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_text() {
        assert_eq!(normalize_text("Hello   World"), "hello world");
    }

    #[test]
    fn test_bbox_continuity() {
        let left = BoundingBox::new(10, 10, 100, 20);
        let right = BoundingBox::new(20, 15, 100, 20);
        assert!(bbox_is_continuous(&left, &right));
    }

    #[test]
    fn test_classify_entity_type() {
        assert_eq!(
            classify_entity_type(Some("Google Chrome"), None),
            "web_page"
        );
        assert_eq!(
            classify_entity_type(Some("Cursor"), None),
            "code_editor_view"
        );
    }
}
