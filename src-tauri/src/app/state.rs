use crate::core::config::Config;
use crate::core::consent::ConsentManager;
use crate::core::database::Database;
use crate::core::input_recorder::InputRecorder;
use crate::core::keyboard_recorder::KeyboardRecorder;
use crate::core::multimodal::MultimodalService;
use crate::core::ocr_processor::OcrProcessor;
use crate::core::os_activity::OsActivityRecorder;
use crate::core::playback_engine::PlaybackEngine;
use crate::core::screen_recorder::ScreenRecorder;
use crate::core::search_engine::SearchEngine;
use crate::core::session_manager::SessionManager;
use crate::core::storage::RecordingStorage;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TimelineData {
    pub sessions: Vec<TimelineSession>,
    pub total_duration: u64,
    pub date_range: DateRange,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DateRange {
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TimelineSession {
    pub id: String,
    pub start_timestamp: i64,
    pub end_timestamp: Option<i64>,
    pub session_type: Option<String>,
    pub applications: Vec<AppUsageSegment>,
    pub activity_intensity: f32,
    pub has_screen_recording: bool,
    pub has_input_recording: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppUsageSegment {
    pub app_name: String,
    pub bundle_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub focus_duration: i64,
    pub color: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DesktopCaptureRuntime {
    pub is_active: bool,
    pub session_id: Option<String>,
    pub started_at: Option<i64>,
    pub display_id: Option<u32>,
    pub display_name: Option<String>,
    pub warnings: Vec<String>,
    pub channel_errors: HashMap<String, String>,
    pub sampler_generation: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopCaptureStatusDto {
    pub is_active: bool,
    pub session_id: Option<String>,
    pub started_at: Option<i64>,
    pub display_id: Option<u32>,
    pub display_name: Option<String>,
    pub channels_enabled: Vec<String>,
    pub warnings: Vec<String>,
    pub missing_permissions: Vec<String>,
    pub resource_profile: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelStatusDto {
    pub channel: String,
    pub enabled: bool,
    pub health: String,
    pub permission_state: String,
    pub last_event_time: Option<i64>,
    pub sample_count: i64,
    pub throughput_per_minute: f32,
    pub last_error: Option<String>,
    pub supports_solo_test: bool,
    pub details: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDataChannelUsageDto {
    pub channel: String,
    pub label: String,
    pub storage_kind: String,
    pub row_count: i64,
    pub disk_bytes: u64,
    pub last_event_time: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDataOverviewDto {
    pub database_path: String,
    pub configured_storage_path: String,
    pub actual_recordings_path: String,
    pub database_size_bytes: u64,
    pub recordings_size_bytes: u64,
    pub total_size_bytes: u64,
    pub disk_total_bytes: u64,
    pub disk_free_bytes: u64,
    pub disk_used_bytes: u64,
    pub source_percent_of_disk: f32,
    pub source_percent_of_free_space: f32,
    pub disk_health: String,
    pub disk_warning: Option<String>,
    pub channels: Vec<CaptureDataChannelUsageDto>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturePreviewRowDto {
    pub timestamp: Option<i64>,
    pub summary: String,
    pub raw_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturePreviewDto {
    pub channel: String,
    pub label: String,
    pub rows: Vec<CapturePreviewRowDto>,
}

pub struct AppState {
    pub db: Arc<Database>,
    pub consent_manager: Arc<ConsentManager>,
    pub config: Arc<Mutex<Config>>,
    pub screen_recorder: Option<ScreenRecorder>,
    pub os_activity_recorder: Option<Arc<OsActivityRecorder>>,
    pub session_manager: Option<Arc<SessionManager>>,
    pub keyboard_recorder: Option<Arc<KeyboardRecorder>>,
    pub input_recorder: Option<Arc<InputRecorder>>,
    pub search_engine: Arc<SearchEngine>,
    pub playback_engine: Option<Arc<PlaybackEngine>>,
    pub storage: Option<Arc<RecordingStorage>>,
    pub ocr_processor: Option<Arc<OcrProcessor>>,
    pub multimodal_service: Option<Arc<MultimodalService>>,
    pub desktop_capture_runtime: Arc<RwLock<DesktopCaptureRuntime>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct RecordingInfo {
    pub session_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: Option<i64>,
    pub segment_count: i64,
    pub total_size_bytes: i64,
    pub total_duration_ms: i64,
    pub frame_count: i64,
}
