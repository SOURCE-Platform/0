use crate::core::database::Database;
use crate::core::ocr_agent_context::{self, AgentSceneSnapshotDto};
use crate::models::activity::AppInfo;
use image::GenericImageView;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_BUCKET_MS: i64 = 30_000;
const DEFAULT_SNAPSHOT_SPAN_MS: i64 = 5_000;
pub const DEFAULT_VISIBLE_WINDOW_MS: i64 = 15 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSnapshotDto {
    pub app_name: String,
    pub bundle_id: String,
    pub process_id: u32,
    pub is_frontmost: bool,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSlice {
    pub id: String,
    pub rail: String,
    pub slice_kind: String,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub title: String,
    pub subtitle: Option<String>,
    pub source: String,
    pub confidence: f32,
    pub session_id: Option<String>,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub interaction_state: Option<String>,
    pub reasons: Vec<String>,
    pub visible_windows: Vec<WindowSnapshotDto>,
    pub ocr_preview: Option<String>,
    pub pii_count: usize,
    pub evidence_frame_path: Option<String>,
    pub storage_bytes: u64,
    pub storage_exact: bool,
    pub row_count: u64,
    pub file_count: u64,
    pub has_detail_view: bool,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRailDto {
    pub id: String,
    pub label: String,
    pub description: String,
    pub confidence_note: String,
    pub slices: Vec<ContextSlice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InteractionSummaryDto {
    pub active_typing_ms: i64,
    pub active_pointer_ms: i64,
    pub passive_viewing_ms: i64,
    pub voice_input_inferred_ms: i64,
    pub mixed_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppMetricSummaryDto {
    pub focused_app_count: usize,
    pub visible_app_count: usize,
    pub total_focus_time_ms: i64,
    pub total_visible_time_ms: i64,
    pub total_interaction_time_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TimelineSummaryDto {
    pub ocr_block_count: usize,
    pub evidence_frame_count: usize,
    pub interaction: InteractionSummaryDto,
    pub app_metrics: AppMetricSummaryDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextTimelineData {
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub now_timestamp: i64,
    pub default_visible_window_ms: i64,
    pub summary: TimelineSummaryDto,
    pub rails: Vec<TimelineRailDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawPayloadDto {
    pub label: String,
    pub raw_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrReconstructionBlockDto {
    pub id: String,
    pub text: String,
    pub confidence: f32,
    pub bounding_box: Option<serde_json::Value>,
    pub pii_entities: Vec<PiiEntityDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrReconstructionDto {
    pub width: u32,
    pub height: u32,
    pub frame_path: Option<String>,
    pub backdrop_available: bool,
    pub blocks: Vec<OcrReconstructionBlockDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSliceDetailDto {
    pub slice: ContextSlice,
    pub rail_label: String,
    pub occurred_at: i64,
    pub duration_ms: i64,
    pub storage_bytes: u64,
    pub storage_exact: bool,
    pub row_count: u64,
    pub file_count: u64,
    pub linked_file_paths: Vec<String>,
    pub focused_app: Option<String>,
    pub focused_bundle_id: Option<String>,
    pub visible_windows: Vec<WindowSnapshotDto>,
    pub interaction_reasons: Vec<String>,
    pub pii_entities: Vec<PiiEntityDto>,
    pub raw_payloads: Vec<RawPayloadDto>,
    pub ocr_reconstruction: Option<OcrReconstructionDto>,
    pub nearby_system_events: Vec<ContextSlice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextInspectorDto {
    pub timestamp: i64,
    pub focused_app: Option<String>,
    pub focused_bundle_id: Option<String>,
    pub visible_windows: Vec<WindowSnapshotDto>,
    pub interaction_state: Option<String>,
    pub interaction_reasons: Vec<String>,
    pub recent_input_state: String,
    pub ocr_text: Vec<String>,
    pub pii_entities: Vec<PiiEntityDto>,
    pub evidence_frame_path: Option<String>,
    pub nearby_system_events: Vec<ContextSlice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsageOverviewItemDto {
    pub app_name: String,
    pub bundle_id: String,
    pub focused_time_ms: i64,
    pub visible_time_ms: i64,
    pub interaction_time_ms: i64,
    pub ocr_hit_count: usize,
    pub recent_segment_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsageOverviewDto {
    pub items: Vec<AppUsageOverviewItemDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiiEntityDto {
    pub id: String,
    pub timestamp: i64,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub entity_type: String,
    pub redacted_preview: String,
    pub confidence: f32,
    pub context_text: String,
    pub bounding_box: Option<serde_json::Value>,
    pub frame_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrReviewItemDto {
    pub id: String,
    pub timestamp: i64,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub text: String,
    pub confidence: f32,
    pub frame_path: Option<String>,
    pub pii_entities: Vec<PiiEntityDto>,
}

#[derive(Debug, Clone, FromRow)]
struct ContextEventRow {
    id: String,
    session_id: Option<String>,
    timestamp: i64,
    channel: String,
    event_type: String,
    source: String,
    confidence: f64,
    payload_json: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct WindowSnapshotRow {
    id: String,
    session_id: Option<String>,
    timestamp: i64,
    frontmost_app_name: Option<String>,
    frontmost_bundle_id: Option<String>,
    visible_windows_json: String,
    confidence: f64,
    source: String,
}

#[derive(Debug, Clone, FromRow)]
struct KeyboardEventSummaryRow {
    timestamp: i64,
    app_name: String,
    window_title: String,
}

#[derive(Debug, Clone, FromRow)]
struct MouseEventSummaryRow {
    timestamp: i64,
    app_name: String,
    window_title: String,
}

#[derive(Debug, Clone, FromRow)]
struct OcrRow {
    id: String,
    session_id: String,
    timestamp: i64,
    frame_path: Option<String>,
    text: String,
    confidence: f64,
    bounding_box: String,
}

#[derive(Debug, Clone, FromRow)]
struct FrameRow {
    session_id: String,
    timestamp: i64,
    file_path: String,
}

#[derive(Debug, Clone, FromRow)]
struct SessionRow {
    id: String,
    start_timestamp: i64,
    end_timestamp: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
struct AppUsageRow {
    session_id: String,
    app_name: String,
    bundle_id: String,
    process_id: i64,
    start_timestamp: i64,
    end_timestamp: Option<i64>,
}

#[derive(Debug, Clone)]
struct OcrEventGroup {
    id: String,
    session_id: String,
    timestamp: i64,
    frame_path: Option<String>,
    blocks: Vec<OcrRow>,
}

#[derive(Debug, Clone)]
struct InferredAppContext {
    app_name: Option<String>,
    bundle_id: Option<String>,
    window_title: Option<String>,
}

pub async fn init_schema(
    db: &Arc<Database>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS context_events (
            id TEXT PRIMARY KEY,
            session_id TEXT,
            timestamp INTEGER NOT NULL,
            channel TEXT NOT NULL,
            event_type TEXT NOT NULL,
            source TEXT NOT NULL,
            confidence REAL NOT NULL,
            payload_json TEXT,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(db.pool())
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS window_snapshots (
            id TEXT PRIMARY KEY,
            session_id TEXT,
            timestamp INTEGER NOT NULL,
            frontmost_app_name TEXT,
            frontmost_bundle_id TEXT,
            visible_windows_json TEXT NOT NULL,
            confidence REAL NOT NULL,
            source TEXT NOT NULL,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(db.pool())
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_context_events_time ON context_events(timestamp)")
        .execute(db.pool())
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_context_events_channel ON context_events(channel)")
        .execute(db.pool())
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_window_snapshots_time ON window_snapshots(timestamp)",
    )
    .execute(db.pool())
    .await?;

    Ok(())
}

pub async fn insert_context_event(
    db: &Arc<Database>,
    session_id: Option<&str>,
    timestamp: i64,
    channel: &str,
    event_type: &str,
    source: &str,
    confidence: f32,
    payload_json: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    sqlx::query(
        r#"
        INSERT INTO context_events (
            id, session_id, timestamp, channel, event_type, source, confidence, payload_json, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(session_id)
    .bind(timestamp)
    .bind(channel)
    .bind(event_type)
    .bind(source)
    .bind(confidence as f64)
    .bind(payload_json)
    .bind(chrono::Utc::now().timestamp_millis())
    .execute(db.pool())
    .await?;

    Ok(())
}

pub async fn insert_window_snapshot(
    db: &Arc<Database>,
    session_id: Option<&str>,
    timestamp: i64,
    frontmost_app_name: Option<&str>,
    frontmost_bundle_id: Option<&str>,
    visible_windows: &[WindowSnapshotDto],
    source: &str,
    confidence: f32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    sqlx::query(
        r#"
        INSERT INTO window_snapshots (
            id, session_id, timestamp, frontmost_app_name, frontmost_bundle_id,
            visible_windows_json, confidence, source, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(session_id)
    .bind(timestamp)
    .bind(frontmost_app_name)
    .bind(frontmost_bundle_id)
    .bind(serde_json::to_string(visible_windows)?)
    .bind(confidence as f64)
    .bind(source)
    .bind(chrono::Utc::now().timestamp_millis())
    .execute(db.pool())
    .await?;

    Ok(())
}

fn pretty_json(value: serde_json::Value) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

fn serialized_len(value: &serde_json::Value) -> u64 {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len() as u64)
        .unwrap_or(0)
}

fn context_event_storage_bytes(row: &ContextEventRow) -> u64 {
    serialized_len(&serde_json::json!({
        "id": row.id,
        "session_id": row.session_id,
        "timestamp": row.timestamp,
        "channel": row.channel,
        "event_type": row.event_type,
        "source": row.source,
        "confidence": row.confidence,
        "payload_json": row.payload_json,
    }))
}

fn session_storage_bytes(row: &SessionRow) -> u64 {
    serialized_len(&serde_json::json!({
        "id": row.id,
        "start_timestamp": row.start_timestamp,
        "end_timestamp": row.end_timestamp,
    }))
}

fn window_snapshot_storage_bytes(row: &WindowSnapshotRow) -> u64 {
    serialized_len(&serde_json::json!({
        "id": row.id,
        "session_id": row.session_id,
        "timestamp": row.timestamp,
        "frontmost_app_name": row.frontmost_app_name,
        "frontmost_bundle_id": row.frontmost_bundle_id,
        "visible_windows_json": row.visible_windows_json,
        "confidence": row.confidence,
        "source": row.source,
    }))
}

fn keyboard_event_storage_bytes(row: &KeyboardEventSummaryRow) -> u64 {
    serialized_len(&serde_json::json!({
        "timestamp": row.timestamp,
        "app_name": row.app_name,
        "window_title": row.window_title,
    }))
}

fn mouse_event_storage_bytes(row: &MouseEventSummaryRow) -> u64 {
    serialized_len(&serde_json::json!({
        "timestamp": row.timestamp,
        "app_name": row.app_name,
        "window_title": row.window_title,
    }))
}

fn ocr_row_storage_bytes(row: &OcrRow) -> u64 {
    serialized_len(&serde_json::json!({
        "id": row.id,
        "session_id": row.session_id,
        "timestamp": row.timestamp,
        "frame_path": row.frame_path,
        "text": row.text,
        "confidence": row.confidence,
        "bounding_box": row.bounding_box,
    }))
}

fn scene_snapshot_storage_bytes(scene: &AgentSceneSnapshotDto) -> u64 {
    serialized_len(&serde_json::json!({
        "scene_id": scene.scene_id,
        "session_id": scene.session_id,
        "timestamp": scene.timestamp,
        "display_id": scene.display_id,
        "frontmost_app_name": scene.frontmost_app_name,
        "frontmost_bundle_id": scene.frontmost_bundle_id,
        "window_title": scene.window_title,
        "trigger_reason": scene.trigger_reason,
        "frame_path": scene.frame_path,
        "frame_width": scene.frame_width,
        "frame_height": scene.frame_height,
        "full_text": scene.full_text,
        "avg_confidence": scene.avg_confidence,
        "text_blocks": scene.text_blocks,
        "pii_entities": scene.pii_entities,
        "raw_source": scene.raw_source,
    }))
}

fn file_size(path: &str) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn get_frame_dimensions(path: &str) -> Option<(u32, u32)> {
    image::open(path).ok().map(|image| image.dimensions())
}

fn agent_pii_entity_to_dto(
    entity: &ocr_agent_context::AgentPiiEntityDto,
    timestamp: i64,
    app_name: Option<&str>,
    window_title: Option<&str>,
    frame_path: Option<&str>,
    id_suffix: usize,
) -> PiiEntityDto {
    PiiEntityDto {
        id: format!(
            "scene-pii-{}-{}-{}",
            entity.entity_type, timestamp, id_suffix
        ),
        timestamp,
        app_name: app_name.map(str::to_string),
        window_title: window_title.map(str::to_string),
        entity_type: entity.entity_type.clone(),
        redacted_preview: entity.redacted_preview.clone(),
        confidence: entity.confidence,
        context_text: entity.context_text.clone(),
        bounding_box: entity
            .bounding_box
            .as_ref()
            .and_then(|bbox| serde_json::to_value(bbox).ok()),
        frame_path: frame_path.map(str::to_string),
    }
}

fn group_ocr_rows(ocr_rows: &[OcrRow]) -> Vec<OcrEventGroup> {
    let mut groups: Vec<OcrEventGroup> = Vec::new();

    for row in ocr_rows {
        let frame_key = row.frame_path.clone().unwrap_or_default();
        if let Some(group) = groups.iter_mut().find(|group| {
            group.session_id == row.session_id
                && group.timestamp == row.timestamp
                && group.frame_path.clone().unwrap_or_default() == frame_key
        }) {
            group.blocks.push(row.clone());
            continue;
        }

        groups.push(OcrEventGroup {
            id: format!("ocr-event-{}-{}", row.session_id, row.timestamp),
            session_id: row.session_id.clone(),
            timestamp: row.timestamp,
            frame_path: row.frame_path.clone(),
            blocks: vec![row.clone()],
        });
    }

    groups
}

pub async fn build_context_timeline(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<ContextTimelineData, Box<dyn std::error::Error + Send + Sync>> {
    let sessions = get_sessions(db, start_timestamp, end_timestamp).await?;
    let events = get_context_events(db, start_timestamp, end_timestamp).await?;
    let snapshots = get_window_snapshots(db, start_timestamp, end_timestamp).await?;
    let keyboard = get_keyboard_events(db, start_timestamp, end_timestamp).await?;
    let mouse = get_mouse_events(db, start_timestamp, end_timestamp).await?;
    let ocr_rows = get_ocr_rows(db, start_timestamp, end_timestamp).await?;
    let frames = get_frame_rows(db, start_timestamp, end_timestamp).await?;
    let ocr_scenes =
        ocr_agent_context::get_scene_snapshots(db, start_timestamp, end_timestamp, None).await?;

    let focus_rail = build_focus_rail(&snapshots, &sessions, end_timestamp);
    let visible_rail = build_visible_windows_rail(&snapshots, &sessions, end_timestamp);
    let system_rail = build_system_rail(&sessions, &events, end_timestamp);
    let interaction_rail = build_interaction_rail(
        &keyboard,
        &mouse,
        &ocr_rows,
        &snapshots,
        start_timestamp,
        end_timestamp,
    );
    let ocr_rail = build_ocr_rail(&ocr_scenes, &snapshots);
    let evidence_rail = build_evidence_rail(&frames);

    let summary = build_summary(
        &focus_rail,
        &visible_rail,
        &interaction_rail,
        &ocr_rail,
        &evidence_rail,
    );
    let now_timestamp = chrono::Utc::now()
        .timestamp_millis()
        .clamp(start_timestamp, end_timestamp);

    Ok(ContextTimelineData {
        start_timestamp,
        end_timestamp,
        now_timestamp,
        default_visible_window_ms: DEFAULT_VISIBLE_WINDOW_MS,
        summary,
        rails: vec![
            system_rail,
            focus_rail,
            visible_rail,
            interaction_rail,
            ocr_rail,
            evidence_rail,
        ],
    })
}

pub async fn get_context_inspector(
    db: &Arc<Database>,
    timestamp: i64,
) -> Result<ContextInspectorDto, Box<dyn std::error::Error + Send + Sync>> {
    let snapshots = get_window_snapshots(db, timestamp - 60_000, timestamp + 60_000).await?;
    let ocr_rows = get_ocr_rows(db, timestamp - 60_000, timestamp + 60_000).await?;
    let keyboard = get_keyboard_events(db, timestamp - 30_000, timestamp + 30_000).await?;
    let mouse = get_mouse_events(db, timestamp - 30_000, timestamp + 30_000).await?;
    let frames = get_frame_rows(db, timestamp - 60_000, timestamp + 60_000).await?;
    let events = get_context_events(db, timestamp - 120_000, timestamp + 120_000).await?;

    let snapshot = snapshots
        .iter()
        .min_by_key(|row| (row.timestamp - timestamp).abs());

    let focused_app = snapshot.and_then(|row| row.frontmost_app_name.clone());
    let focused_bundle_id = snapshot.and_then(|row| row.frontmost_bundle_id.clone());
    let visible_windows = snapshot
        .map(parse_visible_windows)
        .transpose()?
        .unwrap_or_default();

    let interaction_slice = build_interaction_rail(
        &keyboard,
        &mouse,
        &ocr_rows,
        &snapshots,
        timestamp - DEFAULT_BUCKET_MS,
        timestamp + DEFAULT_BUCKET_MS,
    )
    .slices
    .into_iter()
    .next();

    let nearby_ocr: Vec<OcrRow> = ocr_rows
        .into_iter()
        .filter(|row| (row.timestamp - timestamp).abs() <= 30_000)
        .collect();
    let mut pii_entities = Vec::new();
    for row in &nearby_ocr {
        pii_entities.extend(detect_pii_entities(
            row,
            &infer_app_context(db, row.timestamp, Some(&row.session_id)).await?,
        ));
    }

    let nearby_system_events = events
        .into_iter()
        .filter(|row| row.channel == "system" && (row.timestamp - timestamp).abs() <= 120_000)
        .map(|row| event_row_to_slice(&row))
        .collect();

    Ok(ContextInspectorDto {
        timestamp,
        focused_app,
        focused_bundle_id,
        visible_windows,
        interaction_state: interaction_slice
            .as_ref()
            .and_then(|slice| slice.interaction_state.clone()),
        interaction_reasons: interaction_slice
            .as_ref()
            .map(|slice| slice.reasons.clone())
            .unwrap_or_default(),
        recent_input_state: if keyboard.is_empty() && mouse.is_empty() {
            "No direct keyboard or mouse input was observed in the current inspection window."
                .to_string()
        } else {
            format!(
                "{} keyboard events and {} mouse events nearby",
                keyboard.len(),
                mouse.len()
            )
        },
        ocr_text: nearby_ocr.into_iter().map(|row| row.text).collect(),
        pii_entities,
        evidence_frame_path: frames
            .into_iter()
            .min_by_key(|row| (row.timestamp - timestamp).abs())
            .map(|row| row.file_path),
        nearby_system_events,
    })
}

pub async fn get_context_slice_detail(
    db: &Arc<Database>,
    slice_id: &str,
    rail_id: &str,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<ContextSliceDetailDto, Box<dyn std::error::Error + Send + Sync>> {
    let timeline = build_context_timeline(db, start_timestamp, end_timestamp).await?;
    let rail = timeline
        .rails
        .iter()
        .find(|rail| rail.id == rail_id)
        .ok_or_else(|| format!("Unknown rail: {rail_id}"))?;
    let slice = rail
        .slices
        .iter()
        .find(|slice| slice.id == slice_id)
        .cloned()
        .ok_or_else(|| format!("Unknown slice: {slice_id}"))?;

    let snapshots = get_window_snapshots(
        db,
        slice.start_timestamp - 60_000,
        slice.end_timestamp + 60_000,
    )
    .await?;
    let ocr_rows = get_ocr_rows(
        db,
        slice.start_timestamp - 60_000,
        slice.end_timestamp + 60_000,
    )
    .await?;
    let keyboard = get_keyboard_events(
        db,
        slice.start_timestamp - 30_000,
        slice.end_timestamp + 30_000,
    )
    .await?;
    let mouse = get_mouse_events(
        db,
        slice.start_timestamp - 30_000,
        slice.end_timestamp + 30_000,
    )
    .await?;
    let frames = get_frame_rows(
        db,
        slice.start_timestamp - 60_000,
        slice.end_timestamp + 60_000,
    )
    .await?;
    let events = get_context_events(
        db,
        slice.start_timestamp - 120_000,
        slice.end_timestamp + 120_000,
    )
    .await?;
    let sessions = get_sessions(
        db,
        slice.start_timestamp - 120_000,
        slice.end_timestamp + 120_000,
    )
    .await?;
    let context = infer_app_context(db, slice.start_timestamp, slice.session_id.as_deref()).await?;

    let nearby_system_events = events
        .iter()
        .filter(|row| {
            row.channel == "system" && (row.timestamp - slice.start_timestamp).abs() <= 120_000
        })
        .map(event_row_to_slice)
        .collect::<Vec<_>>();

    let mut raw_payloads = Vec::new();
    let mut pii_entities = Vec::new();
    let mut linked_file_paths = Vec::new();
    let mut ocr_reconstruction = None;
    let visible_windows = if slice.visible_windows.is_empty() {
        snapshots
            .iter()
            .min_by_key(|row| (row.timestamp - slice.start_timestamp).abs())
            .map(parse_visible_windows)
            .transpose()?
            .unwrap_or_default()
    } else {
        slice.visible_windows.clone()
    };

    match rail_id {
        "system" => {
            if let Some(session_id) = slice_id
                .strip_prefix("session-start-")
                .or_else(|| slice_id.strip_prefix("session-end-"))
            {
                if let Some(session) = sessions.iter().find(|session| session.id == session_id) {
                    raw_payloads.push(RawPayloadDto {
                        label: "Session row".to_string(),
                        raw_json: pretty_json(serde_json::json!({
                            "id": session.id,
                            "start_timestamp": session.start_timestamp,
                            "end_timestamp": session.end_timestamp,
                        })),
                    });
                }
            } else if let Some(event) = events.iter().find(|row| row.id == slice_id) {
                raw_payloads.push(RawPayloadDto {
                    label: "Context event".to_string(),
                    raw_json: pretty_json(serde_json::json!({
                        "id": event.id,
                        "session_id": event.session_id,
                        "timestamp": event.timestamp,
                        "channel": event.channel,
                        "event_type": event.event_type,
                        "source": event.source,
                        "confidence": event.confidence,
                        "payload_json": event.payload_json.as_ref().and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok()).unwrap_or(serde_json::Value::Null),
                    })),
                });
            }
        }
        "focus" => {
            let matching = snapshots
                .iter()
                .filter(|row| {
                    row.timestamp >= slice.start_timestamp
                        && row.timestamp <= slice.end_timestamp
                        && row.frontmost_app_name == slice.app_name
                })
                .collect::<Vec<_>>();
            for (index, row) in matching.iter().enumerate() {
                raw_payloads.push(RawPayloadDto {
                    label: format!("Snapshot {}", index + 1),
                    raw_json: pretty_json(serde_json::json!({
                        "id": row.id,
                        "session_id": row.session_id,
                        "timestamp": row.timestamp,
                        "frontmost_app_name": row.frontmost_app_name,
                        "frontmost_bundle_id": row.frontmost_bundle_id,
                        "visible_windows": serde_json::from_str::<serde_json::Value>(&row.visible_windows_json).unwrap_or(serde_json::Value::String(row.visible_windows_json.clone())),
                        "confidence": row.confidence,
                        "source": row.source,
                    })),
                });
            }
        }
        "visible_windows" => {
            if let Some(snapshot_id) = slice_id.strip_prefix("visible-") {
                if let Some(row) = snapshots.iter().find(|row| row.id == snapshot_id) {
                    raw_payloads.push(RawPayloadDto {
                        label: "Window snapshot".to_string(),
                        raw_json: pretty_json(serde_json::json!({
                            "id": row.id,
                            "session_id": row.session_id,
                            "timestamp": row.timestamp,
                            "frontmost_app_name": row.frontmost_app_name,
                            "frontmost_bundle_id": row.frontmost_bundle_id,
                            "visible_windows": serde_json::from_str::<serde_json::Value>(&row.visible_windows_json).unwrap_or(serde_json::Value::String(row.visible_windows_json.clone())),
                            "confidence": row.confidence,
                            "source": row.source,
                        })),
                    });
                }
            }
        }
        "interaction" => {
            let keyboard_rows = keyboard
                .iter()
                .filter(|row| {
                    row.timestamp >= slice.start_timestamp && row.timestamp <= slice.end_timestamp
                })
                .collect::<Vec<_>>();
            let mouse_rows = mouse
                .iter()
                .filter(|row| {
                    row.timestamp >= slice.start_timestamp && row.timestamp <= slice.end_timestamp
                })
                .collect::<Vec<_>>();
            let ocr_event_rows = ocr_rows
                .iter()
                .filter(|row| {
                    row.timestamp >= slice.start_timestamp && row.timestamp <= slice.end_timestamp
                })
                .collect::<Vec<_>>();

            raw_payloads.push(RawPayloadDto {
                label: "Interaction bucket".to_string(),
                raw_json: pretty_json(serde_json::json!({
                    "slice_id": slice.id,
                    "start_timestamp": slice.start_timestamp,
                    "end_timestamp": slice.end_timestamp,
                    "interaction_state": slice.interaction_state,
                    "reasons": slice.reasons,
                    "keyboard_events": keyboard_rows.iter().map(|row| serde_json::json!({
                        "timestamp": row.timestamp,
                        "app_name": row.app_name,
                        "window_title": row.window_title,
                    })).collect::<Vec<_>>(),
                    "mouse_events": mouse_rows.iter().map(|row| serde_json::json!({
                        "timestamp": row.timestamp,
                        "app_name": row.app_name,
                        "window_title": row.window_title,
                    })).collect::<Vec<_>>(),
                    "ocr_rows": ocr_event_rows.iter().map(|row| serde_json::json!({
                        "id": row.id,
                        "timestamp": row.timestamp,
                        "text": row.text,
                        "confidence": row.confidence,
                    })).collect::<Vec<_>>(),
                })),
            });
        }
        "ocr" => {
            if let Some(scene) = ocr_agent_context::get_scene_snapshot(db, slice_id).await? {
                let mut reconstruction_blocks = Vec::new();
                let mut max_width = scene.frame_width.unwrap_or(0);
                let mut max_height = scene.frame_height.unwrap_or(0);

                for block in &scene.text_blocks {
                    let bbox = serde_json::to_value(&block.bbox).ok();
                    max_width = max_width.max(block.bbox.x + block.bbox.width);
                    max_height = max_height.max(block.bbox.y + block.bbox.height);
                    reconstruction_blocks.push(OcrReconstructionBlockDto {
                        id: block.block_id.clone(),
                        text: block.text.clone(),
                        confidence: block.confidence,
                        bounding_box: bbox,
                        pii_entities: Vec::new(),
                    });
                }

                if let Some(frame_path) = scene.frame_path.clone() {
                    linked_file_paths.push(frame_path.clone());
                    if let Some((width, height)) = get_frame_dimensions(&frame_path) {
                        max_width = width.max(max_width);
                        max_height = height.max(max_height);
                    }
                }

                pii_entities.extend(scene.pii_entities.iter().enumerate().map(
                    |(index, entity)| {
                        agent_pii_entity_to_dto(
                            entity,
                            scene.timestamp,
                            scene.frontmost_app_name.as_deref(),
                            scene.window_title.as_deref(),
                            scene.frame_path.as_deref(),
                            index + 1000,
                        )
                    },
                ));

                raw_payloads.push(RawPayloadDto {
                    label: "Scene snapshot".to_string(),
                    raw_json: pretty_json(serde_json::to_value(&scene)?),
                });

                let related_spans = ocr_agent_context::get_text_spans(
                    db,
                    scene.timestamp.saturating_sub(5_000),
                    scene.timestamp.saturating_add(5_000),
                    scene.frontmost_app_name.clone(),
                )
                .await?
                .into_iter()
                .filter(|span| {
                    span.scene_ids
                        .iter()
                        .any(|scene_id| scene_id == &scene.scene_id)
                })
                .collect::<Vec<_>>();

                if !related_spans.is_empty() {
                    raw_payloads.push(RawPayloadDto {
                        label: "Related text spans".to_string(),
                        raw_json: pretty_json(serde_json::to_value(&related_spans)?),
                    });
                }

                let related_entities = ocr_agent_context::get_context_entities(
                    db,
                    scene.timestamp.saturating_sub(120_000),
                    scene.timestamp.saturating_add(120_000),
                    scene.frontmost_app_name.clone(),
                    None,
                )
                .await?
                .into_iter()
                .filter(|entity| {
                    entity
                        .scene_ids
                        .iter()
                        .any(|scene_id| scene_id == &scene.scene_id)
                })
                .collect::<Vec<_>>();

                if !related_entities.is_empty() {
                    raw_payloads.push(RawPayloadDto {
                        label: "Related context entities".to_string(),
                        raw_json: pretty_json(serde_json::to_value(&related_entities)?),
                    });
                }

                ocr_reconstruction = Some(OcrReconstructionDto {
                    width: max_width.max(1280),
                    height: max_height.max(720),
                    frame_path: scene.frame_path.clone(),
                    backdrop_available: scene
                        .frame_path
                        .as_ref()
                        .map(|path| Path::new(path).exists())
                        .unwrap_or(false),
                    blocks: reconstruction_blocks,
                });
            } else {
                let ocr_groups = group_ocr_rows(&ocr_rows);
                if let Some(group) = ocr_groups.iter().find(|group| group.id == slice_id) {
                    raw_payloads.push(RawPayloadDto {
                        label: "OCR event (raw fallback)".to_string(),
                        raw_json: pretty_json(serde_json::json!({
                            "id": group.id,
                            "session_id": group.session_id,
                            "timestamp": group.timestamp,
                            "frame_path": group.frame_path,
                            "text_blocks": group.blocks.iter().map(|row| serde_json::json!({
                                "id": row.id,
                                "text": row.text,
                                "confidence": row.confidence,
                                "bounding_box": serde_json::from_str::<serde_json::Value>(&row.bounding_box).unwrap_or(serde_json::Value::String(row.bounding_box.clone())),
                            })).collect::<Vec<_>>(),
                        })),
                    });
                }
            }
        }
        "evidence" => {
            if let Some(frame) = frames
                .iter()
                .find(|row| format!("frame-{}-{}", row.session_id, row.timestamp) == slice_id)
            {
                linked_file_paths.push(frame.file_path.clone());
                raw_payloads.push(RawPayloadDto {
                    label: "Evidence frame".to_string(),
                    raw_json: pretty_json(serde_json::json!({
                        "session_id": frame.session_id,
                        "timestamp": frame.timestamp,
                        "file_path": frame.file_path,
                        "file_size_bytes": file_size(&frame.file_path),
                    })),
                });
            }
        }
        _ => {}
    }

    if linked_file_paths.is_empty() {
        if let Some(path) = slice.evidence_frame_path.clone() {
            linked_file_paths.push(path);
        }
    }

    Ok(ContextSliceDetailDto {
        rail_label: rail.label.clone(),
        occurred_at: slice.start_timestamp,
        duration_ms: (slice.end_timestamp - slice.start_timestamp).max(1),
        storage_bytes: slice.storage_bytes,
        storage_exact: slice.storage_exact,
        row_count: slice.row_count,
        file_count: slice.file_count,
        linked_file_paths,
        focused_app: context.app_name.or_else(|| slice.app_name.clone()),
        focused_bundle_id: context.bundle_id,
        visible_windows,
        interaction_reasons: slice.reasons.clone(),
        pii_entities,
        raw_payloads,
        ocr_reconstruction,
        nearby_system_events,
        slice,
    })
}

pub async fn get_app_usage_overview(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<AppUsageOverviewDto, Box<dyn std::error::Error + Send + Sync>> {
    let sessions = get_sessions(db, start_timestamp, end_timestamp).await?;
    let snapshots = get_window_snapshots(db, start_timestamp, end_timestamp).await?;
    let keyboard = get_keyboard_events(db, start_timestamp, end_timestamp).await?;
    let mouse = get_mouse_events(db, start_timestamp, end_timestamp).await?;
    let ocr_rows = get_ocr_rows(db, start_timestamp, end_timestamp).await?;

    let mut items: HashMap<String, AppUsageOverviewItemDto> = HashMap::new();
    let focus_rail = build_focus_rail(&snapshots, &sessions, end_timestamp);
    for slice in &focus_rail.slices {
        let app_name = slice
            .app_name
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());
        let entry = items
            .entry(app_name.clone())
            .or_insert(AppUsageOverviewItemDto {
                app_name: app_name.clone(),
                bundle_id: String::new(),
                focused_time_ms: 0,
                visible_time_ms: 0,
                interaction_time_ms: 0,
                ocr_hit_count: 0,
                recent_segment_count: 0,
            });
        entry.focused_time_ms += slice.end_timestamp - slice.start_timestamp;
        entry.recent_segment_count += 1;
    }

    for (index, snapshot) in snapshots.iter().enumerate() {
        let visible = parse_visible_windows(snapshot)?;
        let span = snapshots
            .get(index + 1)
            .map(|next| (next.timestamp - snapshot.timestamp).max(1))
            .unwrap_or(DEFAULT_SNAPSHOT_SPAN_MS);
        for window in visible {
            let entry = items
                .entry(window.app_name.clone())
                .or_insert(AppUsageOverviewItemDto {
                    app_name: window.app_name.clone(),
                    bundle_id: window.bundle_id.clone(),
                    focused_time_ms: 0,
                    visible_time_ms: 0,
                    interaction_time_ms: 0,
                    ocr_hit_count: 0,
                    recent_segment_count: 0,
                });
            entry.visible_time_ms += span;
            if entry.bundle_id.is_empty() {
                entry.bundle_id = window.bundle_id;
            }
        }
    }

    for row in keyboard {
        let entry = items
            .entry(row.app_name.clone())
            .or_insert(AppUsageOverviewItemDto {
                app_name: row.app_name.clone(),
                bundle_id: String::new(),
                focused_time_ms: 0,
                visible_time_ms: 0,
                interaction_time_ms: 0,
                ocr_hit_count: 0,
                recent_segment_count: 0,
            });
        entry.interaction_time_ms += 1_000;
    }
    for row in mouse {
        let entry = items
            .entry(row.app_name.clone())
            .or_insert(AppUsageOverviewItemDto {
                app_name: row.app_name.clone(),
                bundle_id: String::new(),
                focused_time_ms: 0,
                visible_time_ms: 0,
                interaction_time_ms: 0,
                ocr_hit_count: 0,
                recent_segment_count: 0,
            });
        entry.interaction_time_ms += 1_000;
    }

    for row in ocr_rows {
        let context = infer_app_context(db, row.timestamp, Some(&row.session_id)).await?;
        if let Some(app_name) = context.app_name {
            let entry = items
                .entry(app_name.clone())
                .or_insert(AppUsageOverviewItemDto {
                    app_name,
                    bundle_id: context.bundle_id.unwrap_or_default(),
                    focused_time_ms: 0,
                    visible_time_ms: 0,
                    interaction_time_ms: 0,
                    ocr_hit_count: 0,
                    recent_segment_count: 0,
                });
            entry.ocr_hit_count += 1;
        }
    }

    let mut items = items.into_values().collect::<Vec<_>>();
    items.sort_by(|a, b| {
        (b.focused_time_ms + b.visible_time_ms + b.interaction_time_ms)
            .cmp(&(a.focused_time_ms + a.visible_time_ms + a.interaction_time_ms))
    });

    Ok(AppUsageOverviewDto { items })
}

pub async fn get_pii_review(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
    entity_type_filter: Option<String>,
    confidence_threshold: Option<f32>,
) -> Result<Vec<PiiEntityDto>, Box<dyn std::error::Error + Send + Sync>> {
    let ocr_rows = get_ocr_rows(db, start_timestamp, end_timestamp).await?;
    let mut items = Vec::new();

    for row in ocr_rows {
        let context = infer_app_context(db, row.timestamp, Some(&row.session_id)).await?;
        let app_name = context.app_name.clone();
        let entities = detect_pii_entities(&row, &context);
        for entity in entities {
            let matches_app = app_filter
                .as_ref()
                .map(|filter| {
                    app_name
                        .as_ref()
                        .map(|name| name == filter)
                        .unwrap_or(false)
                })
                .unwrap_or(true);
            let matches_entity_type = entity_type_filter
                .as_ref()
                .map(|filter| entity.entity_type == *filter)
                .unwrap_or(true);
            let matches_confidence = confidence_threshold
                .map(|threshold| entity.confidence >= threshold)
                .unwrap_or(true);
            if matches_app && matches_entity_type && matches_confidence {
                items.push(entity);
            }
        }
    }

    items.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(items)
}

pub async fn get_ocr_review(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
    query: Option<String>,
    pii_only: bool,
) -> Result<Vec<OcrReviewItemDto>, Box<dyn std::error::Error + Send + Sync>> {
    let ocr_rows = get_ocr_rows(db, start_timestamp, end_timestamp).await?;
    let mut items = Vec::new();

    for row in ocr_rows {
        let context = infer_app_context(db, row.timestamp, Some(&row.session_id)).await?;
        let pii_entities = detect_pii_entities(&row, &context);
        if pii_only && pii_entities.is_empty() {
            continue;
        }

        if let Some(filter) = app_filter.as_ref() {
            if context.app_name.as_ref() != Some(filter) {
                continue;
            }
        }

        if let Some(search) = query.as_ref() {
            if !row.text.to_lowercase().contains(&search.to_lowercase()) {
                continue;
            }
        }

        items.push(OcrReviewItemDto {
            id: row.id.clone(),
            timestamp: row.timestamp,
            app_name: context.app_name,
            window_title: context.window_title,
            text: row.text,
            confidence: row.confidence as f32,
            frame_path: row.frame_path,
            pii_entities,
        });
    }

    items.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(items)
}

pub async fn get_last_event_time_for_table(
    db: &Arc<Database>,
    query: &str,
) -> Result<Option<i64>, Box<dyn std::error::Error + Send + Sync>> {
    let value = sqlx::query_scalar::<_, Option<i64>>(query)
        .fetch_one(db.pool())
        .await?;
    Ok(value)
}

pub async fn get_count_for_query(
    db: &Arc<Database>,
    query: &str,
) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    let value = sqlx::query_scalar::<_, i64>(query)
        .fetch_one(db.pool())
        .await?;
    Ok(value)
}

fn build_system_rail(
    sessions: &[SessionRow],
    events: &[ContextEventRow],
    end_timestamp: i64,
) -> TimelineRailDto {
    let mut slices = Vec::new();

    for session in sessions {
        slices.push(ContextSlice {
            id: format!("session-start-{}", session.id),
            rail: "system".to_string(),
            slice_kind: "event".to_string(),
            start_timestamp: session.start_timestamp,
            end_timestamp: (session.start_timestamp + 5_000).min(end_timestamp),
            title: "Capture session started".to_string(),
            subtitle: Some(session.id.clone()),
            source: "session_manager".to_string(),
            confidence: 1.0,
            session_id: Some(session.id.clone()),
            app_name: None,
            window_title: None,
            interaction_state: None,
            reasons: vec!["A SOURCE desktop capture session was created.".to_string()],
            visible_windows: Vec::new(),
            ocr_preview: None,
            pii_count: 0,
            evidence_frame_path: None,
            storage_bytes: session_storage_bytes(session),
            storage_exact: true,
            row_count: 1,
            file_count: 0,
            has_detail_view: true,
            tags: vec!["session".to_string()],
        });

        if let Some(end) = session.end_timestamp {
            slices.push(ContextSlice {
                id: format!("session-end-{}", session.id),
                rail: "system".to_string(),
                slice_kind: "event".to_string(),
                start_timestamp: end,
                end_timestamp: end + 5_000,
                title: "Capture session ended".to_string(),
                subtitle: Some(session.id.clone()),
                source: "session_manager".to_string(),
                confidence: 1.0,
                session_id: Some(session.id.clone()),
                app_name: None,
                window_title: None,
                interaction_state: None,
                reasons: vec!["The current SOURCE capture session was stopped.".to_string()],
                visible_windows: Vec::new(),
                ocr_preview: None,
                pii_count: 0,
                evidence_frame_path: None,
                storage_bytes: session_storage_bytes(session),
                storage_exact: true,
                row_count: 1,
                file_count: 0,
                has_detail_view: true,
                tags: vec!["session".to_string()],
            });
        }
    }

    for event in events.iter().filter(|event| event.channel == "system") {
        slices.push(event_row_to_slice(event));
    }

    slices.sort_by_key(|slice| slice.start_timestamp);

    TimelineRailDto {
        id: "system".to_string(),
        label: "System".to_string(),
        description: "Capture lifecycle, app launches/quits, and other desktop session transitions that SOURCE can detect today.".to_string(),
        confidence_note: "Mission Control/App Expose and sleep/wake are shown only when explicitly detected. Absence is not faked.".to_string(),
        slices,
    }
}

fn session_end_lookup(sessions: &[SessionRow]) -> HashMap<String, i64> {
    sessions
        .iter()
        .filter_map(|session| session.end_timestamp.map(|end| (session.id.clone(), end)))
        .collect()
}

fn snapshot_span_end(
    snapshot: &WindowSnapshotRow,
    next_timestamp: Option<i64>,
    session_ends: &HashMap<String, i64>,
    fallback_end_timestamp: i64,
) -> i64 {
    let session_end = snapshot
        .session_id
        .as_ref()
        .and_then(|session_id| session_ends.get(session_id).copied());

    match (next_timestamp, session_end) {
        (Some(next), Some(end)) => next.min(end),
        (Some(next), None) => next,
        (None, Some(end)) => end,
        (None, None) => fallback_end_timestamp,
    }
}

fn build_focus_rail(
    snapshots: &[WindowSnapshotRow],
    sessions: &[SessionRow],
    end_timestamp: i64,
) -> TimelineRailDto {
    let mut slices = Vec::new();
    let mut current: Option<ContextSlice> = None;
    let session_ends = session_end_lookup(sessions);

    for (index, snapshot) in snapshots.iter().enumerate() {
        let Some(app_name) = snapshot.frontmost_app_name.clone() else {
            continue;
        };
        let next_timestamp = snapshots.get(index + 1).map(|row| row.timestamp);
        let span_end = snapshot_span_end(snapshot, next_timestamp, &session_ends, end_timestamp);

        match current.as_mut() {
            Some(active) if active.app_name.as_ref() == Some(&app_name) => {
                active.end_timestamp = span_end;
                active.visible_windows = parse_visible_windows(snapshot).unwrap_or_default();
                active.storage_bytes += window_snapshot_storage_bytes(snapshot);
                active.row_count += 1;
            }
            _ => {
                if let Some(previous) = current.take() {
                    slices.push(previous);
                }
                current = Some(ContextSlice {
                    id: format!("focus-{}", snapshot.id),
                    rail: "focus".to_string(),
                    slice_kind: "span".to_string(),
                    start_timestamp: snapshot.timestamp,
                    end_timestamp: span_end,
                    title: app_name.clone(),
                    subtitle: Some(
                        snapshot
                            .frontmost_bundle_id
                            .clone()
                            .unwrap_or_else(|| "Frontmost app".to_string()),
                    ),
                    source: snapshot.source.clone(),
                    confidence: snapshot.confidence as f32,
                    session_id: snapshot.session_id.clone(),
                    app_name: Some(app_name),
                    window_title: None,
                    interaction_state: None,
                    reasons: vec!["Derived from periodic frontmost-app snapshots.".to_string()],
                    visible_windows: parse_visible_windows(snapshot).unwrap_or_default(),
                    ocr_preview: None,
                    pii_count: 0,
                    evidence_frame_path: None,
                    storage_bytes: window_snapshot_storage_bytes(snapshot),
                    storage_exact: false,
                    row_count: 1,
                    file_count: 0,
                    has_detail_view: true,
                    tags: vec!["frontmost".to_string()],
                });
            }
        }
    }

    if let Some(active) = current {
        slices.push(active);
    }

    TimelineRailDto {
        id: "focus".to_string(),
        label: "Focus".to_string(),
        description: "Which app SOURCE believes was frontmost at a given moment.".to_string(),
        confidence_note: "Focus is based on OS snapshots taken during active capture. Historical gaps are left visible instead of backfilled.".to_string(),
        slices,
    }
}

fn build_visible_windows_rail(
    snapshots: &[WindowSnapshotRow],
    sessions: &[SessionRow],
    end_timestamp: i64,
) -> TimelineRailDto {
    let session_ends = session_end_lookup(sessions);
    let slices = snapshots
        .iter()
        .enumerate()
        .map(|(index, snapshot)| {
            let windows = parse_visible_windows(snapshot).unwrap_or_default();
            let span_end = snapshot_span_end(
                snapshot,
                snapshots
                .get(index + 1)
                .map(|row| row.timestamp),
                &session_ends,
                end_timestamp,
            );
            let frontmost = snapshot
                .frontmost_app_name
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            ContextSlice {
                id: format!("visible-{}", snapshot.id),
                rail: "visible_windows".to_string(),
                slice_kind: "span".to_string(),
                start_timestamp: snapshot.timestamp,
                end_timestamp: span_end,
                title: format!("{} visible apps", windows.len()),
                subtitle: Some(format!("Frontmost: {}", frontmost)),
                source: snapshot.source.clone(),
                confidence: snapshot.confidence as f32,
                session_id: snapshot.session_id.clone(),
                app_name: snapshot.frontmost_app_name.clone(),
                window_title: None,
                interaction_state: None,
                reasons: vec!["Visible-window context is best-effort in v1 and uses running-app snapshots as a proxy.".to_string()],
                visible_windows: windows,
                ocr_preview: None,
                pii_count: 0,
                evidence_frame_path: None,
                storage_bytes: window_snapshot_storage_bytes(snapshot),
                storage_exact: false,
                row_count: 1,
                file_count: 0,
                has_detail_view: true,
                tags: vec!["best_effort".to_string(), "visible".to_string()],
            }
        })
        .collect();

    TimelineRailDto {
        id: "visible_windows".to_string(),
        label: "Visible Windows".to_string(),
        description: "Best-effort scene context for what else was on screen alongside the frontmost app.".to_string(),
        confidence_note: "v1 uses running-app snapshots, not a full historical macOS window graph, so this rail is explicitly low-confidence.".to_string(),
        slices,
    }
}

fn build_interaction_rail(
    keyboard: &[KeyboardEventSummaryRow],
    mouse: &[MouseEventSummaryRow],
    ocr_rows: &[OcrRow],
    snapshots: &[WindowSnapshotRow],
    start_timestamp: i64,
    end_timestamp: i64,
) -> TimelineRailDto {
    let mut slices = Vec::new();
    let mut bucket_start = start_timestamp;

    while bucket_start < end_timestamp {
        let bucket_end = (bucket_start + DEFAULT_BUCKET_MS).min(end_timestamp);
        let kb: Vec<_> = keyboard
            .iter()
            .filter(|row| row.timestamp >= bucket_start && row.timestamp < bucket_end)
            .collect();
        let ms: Vec<_> = mouse
            .iter()
            .filter(|row| row.timestamp >= bucket_start && row.timestamp < bucket_end)
            .collect();
        let ocr: Vec<_> = ocr_rows
            .iter()
            .filter(|row| row.timestamp >= bucket_start && row.timestamp < bucket_end)
            .collect();
        let snap = snapshots
            .iter()
            .rev()
            .find(|row| row.timestamp >= bucket_start && row.timestamp < bucket_end)
            .cloned();

        if kb.is_empty() && ms.is_empty() && ocr.is_empty() && snap.is_none() {
            bucket_start = bucket_end;
            continue;
        }

        let (state, reasons, confidence) = if !kb.is_empty() && ms.is_empty() {
            (
                "active_typing".to_string(),
                vec!["Keyboard events present".to_string()],
                1.0,
            )
        } else if kb.is_empty() && !ms.is_empty() {
            (
                "active_pointer".to_string(),
                vec!["Mouse movement or click events present".to_string()],
                1.0,
            )
        } else if !kb.is_empty() && !ms.is_empty() {
            (
                "mixed".to_string(),
                vec![
                    "Keyboard events present".to_string(),
                    "Mouse events present".to_string(),
                ],
                1.0,
            )
        } else {
            let text = ocr
                .iter()
                .map(|row| row.text.to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");
            let voice_inferred = text.contains("recording")
                || text.contains("waveform")
                || text.contains("dictation")
                || text.contains("microphone");
            if voice_inferred {
                (
                    "voice_input_inferred".to_string(),
                    vec!["No keyboard or mouse input was present, but OCR captured recording-like cues.".to_string()],
                    0.55,
                )
            } else {
                (
                    "passive_viewing".to_string(),
                    vec!["No direct input was present, but the screen context was still changing or visible.".to_string()],
                    0.65,
                )
            }
        };

        let app_name = kb
            .first()
            .map(|row| row.app_name.clone())
            .or_else(|| ms.first().map(|row| row.app_name.clone()))
            .or_else(|| snap.as_ref().and_then(|row| row.frontmost_app_name.clone()));
        let window_title = kb
            .first()
            .map(|row| row.window_title.clone())
            .or_else(|| ms.first().map(|row| row.window_title.clone()));
        let storage_bytes = kb
            .iter()
            .map(|row| keyboard_event_storage_bytes(row))
            .sum::<u64>()
            + ms.iter()
                .map(|row| mouse_event_storage_bytes(row))
                .sum::<u64>()
            + ocr
                .iter()
                .map(|row| ocr_row_storage_bytes(row))
                .sum::<u64>();
        let row_count = (kb.len() + ms.len() + ocr.len()) as u64;

        slices.push(ContextSlice {
            id: format!("interaction-{}", bucket_start),
            rail: "interaction".to_string(),
            slice_kind: "span".to_string(),
            start_timestamp: bucket_start,
            end_timestamp: bucket_end,
            title: state.replace('_', " "),
            subtitle: app_name.clone(),
            source: if confidence < 1.0 {
                "hybrid_inference".to_string()
            } else {
                "input_recorder".to_string()
            },
            confidence,
            session_id: None,
            app_name,
            window_title,
            interaction_state: Some(state),
            reasons,
            visible_windows: snap
                .as_ref()
                .map(parse_visible_windows)
                .transpose()
                .unwrap_or_default()
                .unwrap_or_default(),
            ocr_preview: ocr.first().map(|row| preview_text(&row.text)),
            pii_count: ocr
                .iter()
                .map(|row| detect_pii_types_in_text(&row.text).len())
                .sum(),
            evidence_frame_path: None,
            storage_bytes,
            storage_exact: false,
            row_count,
            file_count: 0,
            has_detail_view: true,
            tags: vec!["interaction".to_string()],
        });

        bucket_start = bucket_end;
    }

    TimelineRailDto {
        id: "interaction".to_string(),
        label: "Interaction".to_string(),
        description: "Hard input signals when available, plus clearly-labeled inferred activity states when direct integrations do not exist.".to_string(),
        confidence_note: "Inferred voice input and passive viewing are always labeled as inferences, not authoritative app integrations.".to_string(),
        slices,
    }
}

fn build_ocr_rail(
    ocr_scenes: &[AgentSceneSnapshotDto],
    snapshots: &[WindowSnapshotRow],
) -> TimelineRailDto {
    let slices = ocr_scenes
        .iter()
        .map(|scene| {
            let snapshot = snapshots
                .iter()
                .min_by_key(|snap| (snap.timestamp - scene.timestamp).abs());
            let visible_windows = snapshot
                .map(parse_visible_windows)
                .transpose()
                .unwrap_or_default()
                .unwrap_or_default();
            let storage_bytes = scene_snapshot_storage_bytes(scene);
            ContextSlice {
                id: scene.scene_id.clone(),
                rail: "ocr".to_string(),
                slice_kind: "event".to_string(),
                start_timestamp: scene.timestamp,
                end_timestamp: scene.timestamp + 5_000,
                title: preview_text(&scene.full_text),
                subtitle: Some(format!(
                    "{} text blocks · {}",
                    scene.block_count, scene.trigger_reason
                )),
                source: "ocr_scene_snapshot".to_string(),
                confidence: scene.avg_confidence,
                session_id: Some(scene.session_id.clone()),
                app_name: scene
                    .frontmost_app_name
                    .clone()
                    .or_else(|| snapshot.and_then(|snap| snap.frontmost_app_name.clone())),
                window_title: scene.window_title.clone(),
                interaction_state: None,
                reasons: vec![format!(
                    "OCR captured this scene because {} triggered a new OCR pass.",
                    scene.trigger_reason.replace('_', " ")
                )],
                visible_windows,
                ocr_preview: Some(scene.full_text.clone()),
                pii_count: scene.pii_entities.len(),
                evidence_frame_path: scene.frame_path.clone(),
                storage_bytes,
                storage_exact: true,
                row_count: scene.raw_source.raw_row_ids.len() as u64,
                file_count: scene.frame_path.as_ref().map(|_| 1).unwrap_or(0),
                has_detail_view: true,
                tags: vec!["ocr".to_string(), scene.trigger_reason.clone()],
            }
        })
        .collect();

    TimelineRailDto {
        id: "ocr".to_string(),
        label: "OCR / Text".to_string(),
        description: "Captured text blocks that can later be searched, reviewed, and linked back to app context.".to_string(),
        confidence_note: "If OCR is unavailable or overloaded, this rail will degrade cleanly instead of inventing text.".to_string(),
        slices,
    }
}

fn build_evidence_rail(frames: &[FrameRow]) -> TimelineRailDto {
    let slices = frames
        .iter()
        .map(|row| ContextSlice {
            id: format!("frame-{}-{}", row.session_id, row.timestamp),
            rail: "evidence".to_string(),
            slice_kind: "event".to_string(),
            start_timestamp: row.timestamp,
            end_timestamp: row.timestamp + 1_000,
            title: "Retained evidence frame".to_string(),
            subtitle: Some(row.session_id.clone()),
            source: "screen_recorder".to_string(),
            confidence: 1.0,
            session_id: Some(row.session_id.clone()),
            app_name: None,
            window_title: None,
            interaction_state: None,
            reasons: vec![
                "This is a stored frame anchor that can support playback or OCR review."
                    .to_string(),
            ],
            visible_windows: Vec::new(),
            ocr_preview: None,
            pii_count: 0,
            evidence_frame_path: Some(row.file_path.clone()),
            storage_bytes: file_size(&row.file_path),
            storage_exact: true,
            row_count: 1,
            file_count: 1,
            has_detail_view: true,
            tags: vec!["evidence".to_string()],
        })
        .collect();

    TimelineRailDto {
        id: "evidence".to_string(),
        label: "Evidence".to_string(),
        description: "Retained keyframes and media anchors. In real mode these support review rather than acting as the primary navigation object.".to_string(),
        confidence_note: "Evidence can be absent while OCR, input, and system context still remain available.".to_string(),
        slices,
    }
}

fn build_summary(
    focus_rail: &TimelineRailDto,
    visible_rail: &TimelineRailDto,
    interaction_rail: &TimelineRailDto,
    ocr_rail: &TimelineRailDto,
    evidence_rail: &TimelineRailDto,
) -> TimelineSummaryDto {
    let mut focused_apps = HashMap::new();
    let mut visible_apps = HashMap::new();
    let mut interaction = InteractionSummaryDto::default();

    let total_focus_time_ms = focus_rail
        .slices
        .iter()
        .map(|slice| {
            if let Some(app) = slice.app_name.as_ref() {
                focused_apps.insert(app.clone(), true);
            }
            slice.end_timestamp - slice.start_timestamp
        })
        .sum();

    let total_visible_time_ms = visible_rail
        .slices
        .iter()
        .map(|slice| {
            for window in &slice.visible_windows {
                visible_apps.insert(window.app_name.clone(), true);
            }
            slice.end_timestamp - slice.start_timestamp
        })
        .sum();

    for slice in &interaction_rail.slices {
        let duration = slice.end_timestamp - slice.start_timestamp;
        match slice.interaction_state.as_deref() {
            Some("active_typing") => interaction.active_typing_ms += duration,
            Some("active_pointer") => interaction.active_pointer_ms += duration,
            Some("voice_input_inferred") => interaction.voice_input_inferred_ms += duration,
            Some("mixed") => interaction.mixed_ms += duration,
            _ => interaction.passive_viewing_ms += duration,
        }
    }

    let total_interaction_time_ms = interaction.active_typing_ms
        + interaction.active_pointer_ms
        + interaction.voice_input_inferred_ms
        + interaction.mixed_ms
        + interaction.passive_viewing_ms;

    TimelineSummaryDto {
        ocr_block_count: ocr_rail.slices.len(),
        evidence_frame_count: evidence_rail.slices.len(),
        interaction,
        app_metrics: AppMetricSummaryDto {
            focused_app_count: focused_apps.len(),
            visible_app_count: visible_apps.len(),
            total_focus_time_ms,
            total_visible_time_ms,
            total_interaction_time_ms,
        },
    }
}

fn event_row_to_slice(row: &ContextEventRow) -> ContextSlice {
    let payload = row
        .payload_json
        .as_ref()
        .and_then(|payload| serde_json::from_str::<serde_json::Value>(payload).ok());
    let title = payload
        .as_ref()
        .and_then(|value| {
            value
                .get("title")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
        })
        .unwrap_or_else(|| row.event_type.replace('_', " "));
    let subtitle = payload.as_ref().and_then(|value| {
        value
            .get("subtitle")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
    });

    ContextSlice {
        id: row.id.clone(),
        rail: row.channel.clone(),
        slice_kind: "event".to_string(),
        start_timestamp: row.timestamp,
        end_timestamp: row.timestamp + 5_000,
        title,
        subtitle,
        source: row.source.clone(),
        confidence: row.confidence as f32,
        session_id: row.session_id.clone(),
        app_name: payload.as_ref().and_then(|value| {
            value
                .get("app_name")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
        }),
        window_title: payload.as_ref().and_then(|value| {
            value
                .get("window_title")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
        }),
        interaction_state: None,
        reasons: vec!["Persisted as an explicit desktop context event.".to_string()],
        visible_windows: Vec::new(),
        ocr_preview: None,
        pii_count: 0,
        evidence_frame_path: None,
        storage_bytes: context_event_storage_bytes(row),
        storage_exact: true,
        row_count: 1,
        file_count: 0,
        has_detail_view: true,
        tags: vec![row.channel.clone(), row.event_type.clone()],
    }
}

fn parse_visible_windows(
    row: &WindowSnapshotRow,
) -> Result<Vec<WindowSnapshotDto>, Box<dyn std::error::Error + Send + Sync>> {
    let windows = serde_json::from_str::<Vec<WindowSnapshotDto>>(&row.visible_windows_json)?;
    Ok(windows)
}

fn take_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn preview_text(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized_len = normalized.chars().count();
    if normalized_len <= 72 {
        normalized
    } else {
        format!("{}...", take_chars(&normalized, 72))
    }
}

fn redact_match(value: &str) -> String {
    if value.chars().count() <= 4 {
        "••••".to_string()
    } else {
        format!("{}••••", take_chars(value, 4))
    }
}

fn detect_pii_types_in_text(text: &str) -> Vec<String> {
    detect_pii_spans(text)
        .into_iter()
        .map(|(entity_type, _)| entity_type)
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
        (
            "ip_address",
            Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").unwrap(),
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

fn detect_pii_entities(row: &OcrRow, context: &InferredAppContext) -> Vec<PiiEntityDto> {
    let bbox = serde_json::from_str::<serde_json::Value>(&row.bounding_box).ok();
    detect_pii_spans(&row.text)
        .into_iter()
        .enumerate()
        .map(|(index, (entity_type, matched))| PiiEntityDto {
            id: format!("{}-{}-{}", row.id, entity_type, index),
            timestamp: row.timestamp,
            app_name: context.app_name.clone(),
            window_title: context.window_title.clone(),
            entity_type,
            redacted_preview: redact_match(&matched),
            confidence: 0.72,
            context_text: row.text.clone(),
            bounding_box: bbox.clone(),
            frame_path: row.frame_path.clone(),
        })
        .collect()
}

async fn infer_app_context(
    db: &Arc<Database>,
    timestamp: i64,
    session_id: Option<&str>,
) -> Result<InferredAppContext, Box<dyn std::error::Error + Send + Sync>> {
    let snapshot = sqlx::query_as::<_, WindowSnapshotRow>(
        r#"
        SELECT id, session_id, timestamp, frontmost_app_name, frontmost_bundle_id, visible_windows_json, confidence, source
        FROM window_snapshots
        WHERE timestamp <= ?
        ORDER BY timestamp DESC
        LIMIT 1
        "#,
    )
    .bind(timestamp)
    .fetch_optional(db.pool())
    .await?;

    if let Some(snapshot) = snapshot {
        return Ok(InferredAppContext {
            app_name: snapshot.frontmost_app_name,
            bundle_id: snapshot.frontmost_bundle_id,
            window_title: None,
        });
    }

    let session_clause = if session_id.is_some() {
        "AND session_id = ?"
    } else {
        ""
    };
    let keyboard_query = format!(
        r#"
        SELECT timestamp, app_name, window_title
        FROM keyboard_events
        WHERE timestamp <= ? {}
        ORDER BY timestamp DESC
        LIMIT 1
        "#,
        session_clause
    );
    let mut keyboard_query_builder =
        sqlx::query_as::<_, KeyboardEventSummaryRow>(&keyboard_query).bind(timestamp);
    if let Some(session_id) = session_id {
        keyboard_query_builder = keyboard_query_builder.bind(session_id);
    }
    if let Some(event) = keyboard_query_builder.fetch_optional(db.pool()).await? {
        return Ok(InferredAppContext {
            app_name: Some(event.app_name),
            bundle_id: None,
            window_title: Some(event.window_title),
        });
    }

    let app_usage = sqlx::query_as::<_, AppUsageRow>(
        r#"
        SELECT session_id, app_name, bundle_id, process_id, start_timestamp, end_timestamp
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

    if let Some(app) = app_usage {
        return Ok(InferredAppContext {
            app_name: Some(app.app_name),
            bundle_id: Some(app.bundle_id),
            window_title: None,
        });
    }

    Ok(InferredAppContext {
        app_name: None,
        bundle_id: None,
        window_title: None,
    })
}

async fn get_sessions(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<Vec<SessionRow>, Box<dyn std::error::Error + Send + Sync>> {
    let sessions = sqlx::query_as::<_, SessionRow>(
        r#"
        SELECT id, start_timestamp, end_timestamp
        FROM sessions
        WHERE start_timestamp <= ?
          AND (end_timestamp IS NULL OR end_timestamp >= ?)
        ORDER BY start_timestamp ASC
        "#,
    )
    .bind(end_timestamp)
    .bind(start_timestamp)
    .fetch_all(db.pool())
    .await?;
    Ok(sessions)
}

async fn get_context_events(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<Vec<ContextEventRow>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, ContextEventRow>(
        r#"
        SELECT id, session_id, timestamp, channel, event_type, source, confidence, payload_json
        FROM context_events
        WHERE timestamp >= ? AND timestamp <= ?
        ORDER BY timestamp ASC
        "#,
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await?;
    Ok(rows)
}

async fn get_window_snapshots(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<Vec<WindowSnapshotRow>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, WindowSnapshotRow>(
        r#"
        SELECT id, session_id, timestamp, frontmost_app_name, frontmost_bundle_id, visible_windows_json, confidence, source
        FROM window_snapshots
        WHERE timestamp >= ? AND timestamp <= ?
        ORDER BY timestamp ASC
        "#,
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await?;
    Ok(rows)
}

async fn get_keyboard_events(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<Vec<KeyboardEventSummaryRow>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, KeyboardEventSummaryRow>(
        r#"
        SELECT timestamp, app_name, window_title
        FROM keyboard_events
        WHERE timestamp >= ? AND timestamp <= ?
        ORDER BY timestamp ASC
        "#,
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await?;
    Ok(rows)
}

async fn get_mouse_events(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<Vec<MouseEventSummaryRow>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, MouseEventSummaryRow>(
        r#"
        SELECT timestamp, app_name, window_title
        FROM mouse_events
        WHERE timestamp >= ? AND timestamp <= ?
        ORDER BY timestamp ASC
        "#,
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await?;
    Ok(rows)
}

async fn get_ocr_rows(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<Vec<OcrRow>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, OcrRow>(
        r#"
        SELECT id, session_id, timestamp, frame_path, text, confidence, bounding_box
        FROM ocr_results
        WHERE timestamp >= ? AND timestamp <= ?
        ORDER BY timestamp ASC
        LIMIT 500
        "#,
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await?;
    Ok(rows)
}

async fn get_frame_rows(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<Vec<FrameRow>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, FrameRow>(
        r#"
        SELECT session_id, timestamp, file_path
        FROM frames
        WHERE timestamp >= ? AND timestamp <= ?
        ORDER BY timestamp ASC
        LIMIT 120
        "#,
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await
    .unwrap_or_default();
    Ok(rows)
}

pub fn app_infos_to_visible_windows(
    frontmost: Option<&AppInfo>,
    running_apps: &[AppInfo],
) -> Vec<WindowSnapshotDto> {
    running_apps
        .iter()
        .map(|app| WindowSnapshotDto {
            app_name: app.name.clone(),
            bundle_id: app.bundle_id.clone(),
            process_id: app.process_id,
            is_frontmost: frontmost
                .map(|item| item.process_id == app.process_id)
                .unwrap_or(false),
            confidence: if frontmost
                .map(|item| item.process_id == app.process_id)
                .unwrap_or(false)
            {
                0.95
            } else {
                0.45
            },
        })
        .collect()
}
