use crate::models::ocr::BoundingBox;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredOcrResult {
    pub id: String,
    pub session_id: Uuid,
    pub timestamp: i64,
    pub frame_path: Option<PathBuf>,
    pub text: String,
    pub confidence: f32,
    pub bounding_box: BoundingBox,
    pub language: String,
    pub processing_time_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub session_id: Uuid,
    pub timestamp: i64,
    pub text: String,
    pub confidence: f32,
    pub frame_path: Option<PathBuf>,
    pub bounding_box: BoundingBox,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrStats {
    pub frames_processed: u64,
    pub text_blocks_extracted: u64,
    pub average_processing_time_ms: f64,
    pub average_confidence: f64,
    pub total_text_length: u64,
}
