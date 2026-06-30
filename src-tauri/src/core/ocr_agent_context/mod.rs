use crate::models::ocr::BoundingBox;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;

pub(super) const SPAN_MAX_GAP_MS: i64 = 15_000;
pub(super) const ENTITY_MAX_GAP_MS: i64 = 120_000;
pub(super) const BBOX_POSITION_TOLERANCE: i64 = 80;
pub(super) const APP_CONTEXT_LOOKBACK_MS: i64 = 60_000;

mod app_context;
mod entities;
mod mappings;
mod pii;
mod queries;
mod scene_index;
mod spans;

pub use queries::{
    get_activity_episode, get_context_entities, get_ocr_agent_summary, get_scene_snapshot,
    get_scene_snapshots, get_text_spans, search_agent_context,
};
pub use scene_index::{delete_all_derived, delete_derived_for_session, reindex_processed_result};

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
pub(super) struct WindowSnapshotContextRow {
    pub id: String,
    pub timestamp: i64,
    pub frontmost_app_name: Option<String>,
    pub frontmost_bundle_id: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct KeyboardContextRow {
    pub timestamp: i64,
    pub app_name: String,
    pub window_title: String,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct AppUsageContextRow {
    pub app_name: String,
    pub bundle_id: String,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct SceneSnapshotRow {
    pub scene_id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub display_id: Option<i64>,
    pub frontmost_app_name: Option<String>,
    pub frontmost_bundle_id: Option<String>,
    pub window_title: Option<String>,
    pub trigger_reason: String,
    pub frame_path: Option<String>,
    pub frame_width: Option<i64>,
    pub frame_height: Option<i64>,
    pub full_text: String,
    pub avg_confidence: f64,
    pub block_count: i64,
    pub text_blocks_json: String,
    pub pii_entities_json: String,
    pub linked_context_json: String,
    pub raw_source_json: String,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct TextSpanRow {
    pub text_span_id: String,
    pub session_id: String,
    pub canonical_text: String,
    pub normalized_text: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub duration_ms: i64,
    pub scene_ids_json: String,
    pub frontmost_app_name: Option<String>,
    pub frontmost_bundle_id: Option<String>,
    pub window_title: Option<String>,
    pub avg_confidence: f64,
    pub bbox_union_json: String,
    pub occurrence_count: i64,
    pub was_partial_match: i64,
    pub pii_entities_json: String,
    pub raw_source_json: String,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct ContextEntityRow {
    pub entity_id: String,
    pub session_id: String,
    pub entity_type: String,
    pub frontmost_app_name: Option<String>,
    pub frontmost_bundle_id: Option<String>,
    pub window_title: Option<String>,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub duration_ms: i64,
    pub scene_ids_json: String,
    pub text_span_ids_json: String,
    pub title_hint: Option<String>,
    pub summary_text: String,
    pub dominant_terms_json: String,
    pub pii_entity_counts_json: String,
    pub raw_source_json: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Default)]
pub(super) struct InferredAppContext {
    pub app_name: Option<String>,
    pub bundle_id: Option<String>,
    pub window_title: Option<String>,
    pub visible_window_snapshot_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct MutableTextSpan {
    pub canonical_text: String,
    pub normalized_text: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub scene_ids: Vec<String>,
    pub frontmost_app_name: Option<String>,
    pub frontmost_bundle_id: Option<String>,
    pub window_title: Option<String>,
    pub confidence_sum: f32,
    pub confidence_count: usize,
    pub bbox_union: BoundingBox,
    pub occurrence_count: usize,
    pub was_partial_match: bool,
    pub pii_entities: Vec<AgentPiiEntityDto>,
    pub raw_row_ids: Vec<String>,
}
