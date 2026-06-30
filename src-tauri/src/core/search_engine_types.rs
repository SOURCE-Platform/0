use crate::models::ocr::BoundingBox;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    #[serde(default)]
    pub filters: SearchFilters,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

pub fn default_limit() -> u32 {
    50
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchFilters {
    pub session_ids: Option<Vec<Uuid>>,
    pub date_range: Option<TimeRange>,
    pub min_confidence: Option<f32>,
    pub app_names: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub session_id: Uuid,
    pub timestamp: i64,
    pub text_snippet: String,
    pub full_text: String,
    pub confidence: f32,
    pub bounding_box: BoundingBox,
    pub frame_path: Option<PathBuf>,
    pub app_context: Option<String>,
    pub relevance_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub results: Vec<SearchResult>,
    pub total_count: u32,
    pub query_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultWithContext {
    pub result: SearchResult,
    pub before_text: String,
    pub after_text: String,
}
