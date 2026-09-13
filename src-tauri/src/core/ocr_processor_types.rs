use crate::core::ocr_storage::CapturedApp;
use crate::models::ocr::BoundingBox;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrProcessorConfig {
    pub enabled: bool,
    pub interval_seconds: u32,
    pub batch_size: usize,
    pub skip_static_frames: bool,
    pub max_queue_size: usize,
}

impl Default for OcrProcessorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_seconds: 60,
            batch_size: 5,
            skip_static_frames: true,
            max_queue_size: 100,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OcrJob {
    pub session_id: Uuid,
    pub frame_path: PathBuf,
    pub timestamp: i64,
    pub display_id: Option<u32>,
    pub trigger_reason: String,
    pub motion_regions: Vec<BoundingBox>,
    pub frontmost_app: Option<CapturedApp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OcrMetrics {
    pub frames_processed: u64,
    pub text_blocks_extracted: u64,
    pub total_processing_time_ms: u64,
    pub average_processing_time_ms: f64,
    pub queue_size: usize,
}
