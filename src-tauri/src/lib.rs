pub mod core;
pub mod models;
pub mod platform;

use chrono;
use core::command_analyzer::{Command, CommandAnalyzer, CommandStats};
use core::config::{Config, ResourceProfile};
use core::consent::{ConsentManager, Feature};
use core::context_timeline::{
    self, AppUsageOverviewDto, ContextInspectorDto, ContextSliceDetailDto,
    ContextTimelineData as DesktopContextTimelineData, OcrReviewItemDto, PiiEntityDto,
    WindowSnapshotDto,
};
use core::database::Database;
use core::input_recorder::InputRecorder;
use core::input_storage::{InputTimeline, TimeRange};
use core::keyboard_recorder::KeyboardRecorder;
use core::ocr_agent_context::{
    self, ActivityEpisodeDto, AgentContextEntityDto, AgentContextSearchResultDto,
    AgentSceneSnapshotDto, AgentTextSpanDto, OcrAgentSummaryDto,
};
use core::ocr_engine::{OcrConfig, OcrEngine};
use core::ocr_processor::{OcrProcessor, OcrProcessorConfig};
use core::ocr_storage::OcrStorage;
use core::ocr_trigger_signals::OcrTriggerSignals;
use core::os_activity::{AppUsageStats, OsActivityRecorder};
use core::playback_engine::{PlaybackEngine, PlaybackInfo, SeekInfo};
use core::screen_recorder::{RecordingStatus, ScreenRecorder};
use core::search_engine::{SearchEngine, SearchFilters, SearchQuery, SearchResults};
use core::session_manager::{Session, SessionConfig, SessionManager, SessionMetrics};
use core::storage::RecordingStorage;
use models::activity::AppInfo;
use models::capture::Display;
use models::input::{KeyboardEvent, KeyboardStats, MouseEvent};
use platform::get_platform;
use sqlx::Row;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};
use std::{fs, io};
use tauri::{Manager, State};
use tokio::sync::RwLock;
use uuid::Uuid;

// Timeline data structures
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

// Application state
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

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

// Consent management commands
#[tauri::command]
async fn check_consent_status(feature: String, state: State<'_, AppState>) -> Result<bool, String> {
    let feature = Feature::from_string(&feature).map_err(|e| format!("Invalid feature: {}", e))?;

    state
        .consent_manager
        .is_consent_granted(feature)
        .await
        .map_err(|e| format!("Failed to check consent: {}", e))
}

#[tauri::command]
async fn request_consent(feature: String, state: State<'_, AppState>) -> Result<(), String> {
    let feature = Feature::from_string(&feature).map_err(|e| format!("Invalid feature: {}", e))?;

    state
        .consent_manager
        .grant_consent(feature)
        .await
        .map_err(|e| format!("Failed to grant consent: {}", e))
}

#[tauri::command]
async fn revoke_consent(feature: String, state: State<'_, AppState>) -> Result<(), String> {
    let feature = Feature::from_string(&feature).map_err(|e| format!("Invalid feature: {}", e))?;

    state
        .consent_manager
        .revoke_consent(feature)
        .await
        .map_err(|e| format!("Failed to revoke consent: {}", e))
}

#[tauri::command]
async fn get_all_consents(state: State<'_, AppState>) -> Result<HashMap<String, bool>, String> {
    let consents = state
        .consent_manager
        .get_all_consents()
        .await
        .map_err(|e| format!("Failed to get consents: {}", e))?;

    // Convert Feature keys to strings for JSON serialization
    let mut string_consents = HashMap::new();
    for (feature, granted) in consents {
        string_consents.insert(feature.to_db_string().to_string(), granted);
    }

    Ok(string_consents)
}

// Configuration management commands
#[tauri::command]
fn get_config(state: State<'_, AppState>) -> Result<Config, String> {
    let config = state
        .config
        .lock()
        .map_err(|e| format!("Failed to lock config: {}", e))?;

    Ok(config.clone())
}

#[tauri::command]
fn update_config(config: Config, state: State<'_, AppState>) -> Result<(), String> {
    // Validate config
    config
        .validate()
        .map_err(|e| format!("Invalid configuration: {}", e))?;

    // Update in-memory config
    let mut current_config = state
        .config
        .lock()
        .map_err(|e| format!("Failed to lock config: {}", e))?;

    *current_config = config.clone();

    // Save to disk
    config
        .save()
        .map_err(|e| format!("Failed to save config: {}", e))?;

    Ok(())
}

#[tauri::command]
fn reset_config(state: State<'_, AppState>) -> Result<Config, String> {
    let default_config = Config::reset().map_err(|e| format!("Failed to reset config: {}", e))?;

    // Update in-memory config
    let mut current_config = state
        .config
        .lock()
        .map_err(|e| format!("Failed to lock config: {}", e))?;

    *current_config = default_config.clone();

    Ok(default_config)
}

// Screen recording commands
#[tauri::command]
async fn get_available_displays(state: State<'_, AppState>) -> Result<Vec<Display>, String> {
    let recorder = state
        .screen_recorder
        .as_ref()
        .ok_or("Screen recorder not initialized")?;

    recorder
        .get_available_displays()
        .await
        .map_err(|e| format!("Failed to get displays: {}", e))
}

#[tauri::command]
async fn start_screen_recording(display_id: u32, state: State<'_, AppState>) -> Result<(), String> {
    let recorder = state
        .screen_recorder
        .as_ref()
        .ok_or("Screen recorder not initialized")?;

    recorder
        .start_recording(display_id)
        .await
        .map_err(|e| format!("Failed to start recording: {}", e))
}

#[tauri::command]
async fn stop_screen_recording(state: State<'_, AppState>) -> Result<(), String> {
    let recorder = state
        .screen_recorder
        .as_ref()
        .ok_or("Screen recorder not initialized")?;

    recorder
        .stop_recording()
        .await
        .map_err(|e| format!("Failed to stop recording: {}", e))
}

#[tauri::command]
async fn get_recording_status(state: State<'_, AppState>) -> Result<RecordingStatus, String> {
    let recorder = state
        .screen_recorder
        .as_ref()
        .ok_or("Screen recorder not initialized")?;

    recorder
        .get_status()
        .await
        .map_err(|e| format!("Failed to get status: {}", e))
}

fn enabled_channels_from_config(config: &Config) -> Vec<String> {
    let channels = &config.capture_channels;
    let all = [
        ("system", channels.system),
        ("focus", channels.focus),
        ("visible_windows", channels.visible_windows),
        ("ocr", channels.ocr),
        ("keyboard", channels.keyboard),
        ("mouse", channels.mouse),
        ("screen_frames", channels.screen_frames),
        ("audio_future", channels.audio_future),
        ("camera_future", channels.camera_future),
        ("sensor_future", channels.sensor_future),
    ];

    all.into_iter()
        .filter(|(_, enabled)| *enabled)
        .map(|(name, _)| name.to_string())
        .collect()
}

fn interval_for_profile(profile: &ResourceProfile) -> u64 {
    match profile {
        ResourceProfile::Minimal => 15,
        ResourceProfile::Balanced => 5,
        ResourceProfile::HighFidelity => 2,
    }
}

async fn channel_permission_state(consent_manager: &Arc<ConsentManager>, channel: &str) -> String {
    let feature = match channel {
        "system" | "focus" | "visible_windows" => Some(Feature::OsActivity),
        "keyboard" => Some(Feature::KeyboardRecording),
        "mouse" => Some(Feature::MouseRecording),
        "screen_frames" => Some(Feature::ScreenRecording),
        "ocr" => Some(Feature::ScreenRecording),
        _ => None,
    };

    match feature {
        Some(feature) => match consent_manager.is_consent_granted(feature).await {
            Ok(true) => "granted".to_string(),
            Ok(false) => "missing".to_string(),
            Err(_) => "unknown".to_string(),
        },
        None => "not_required".to_string(),
    }
}

async fn build_desktop_capture_status(state: &AppState) -> Result<DesktopCaptureStatusDto, String> {
    let runtime = state.desktop_capture_runtime.read().await.clone();
    let config = state
        .config
        .lock()
        .map_err(|e| format!("Failed to lock config: {}", e))?
        .clone();
    let enabled_channels = enabled_channels_from_config(&config);
    let mut missing_permissions = Vec::new();
    for channel in &enabled_channels {
        if channel_permission_state(&state.consent_manager, channel).await == "missing" {
            missing_permissions.push(channel.clone());
        }
    }

    Ok(DesktopCaptureStatusDto {
        is_active: runtime.is_active,
        session_id: runtime.session_id,
        started_at: runtime.started_at,
        display_id: runtime.display_id,
        display_name: runtime.display_name,
        channels_enabled: enabled_channels,
        warnings: runtime.warnings,
        missing_permissions,
        resource_profile: format!("{:?}", config.resource_profile),
    })
}

struct CaptureChannelMeta {
    channel: &'static str,
    label: &'static str,
    storage_kind: &'static str,
    count_query: &'static str,
    last_query: &'static str,
}

const CAPTURE_CHANNELS: [CaptureChannelMeta; 7] = [
    CaptureChannelMeta {
        channel: "system",
        label: "OS / session events",
        storage_kind: "database",
        count_query: "SELECT COUNT(*) FROM context_events WHERE channel = 'system'",
        last_query: "SELECT MAX(timestamp) FROM context_events WHERE channel = 'system'",
    },
    CaptureChannelMeta {
        channel: "focus",
        label: "Focus and running apps",
        storage_kind: "database",
        count_query: "SELECT COUNT(*) FROM context_events WHERE channel = 'focus'",
        last_query: "SELECT MAX(timestamp) FROM context_events WHERE channel = 'focus'",
    },
    CaptureChannelMeta {
        channel: "visible_windows",
        label: "Visible windows snapshots",
        storage_kind: "database",
        count_query: "SELECT COUNT(*) FROM window_snapshots",
        last_query: "SELECT MAX(timestamp) FROM window_snapshots",
    },
    CaptureChannelMeta {
        channel: "keyboard",
        label: "Keyboard activity",
        storage_kind: "database",
        count_query: "SELECT COUNT(*) FROM keyboard_events",
        last_query: "SELECT MAX(timestamp) FROM keyboard_events",
    },
    CaptureChannelMeta {
        channel: "mouse",
        label: "Mouse activity",
        storage_kind: "database",
        count_query: "SELECT COUNT(*) FROM mouse_events",
        last_query: "SELECT MAX(timestamp) FROM mouse_events",
    },
    CaptureChannelMeta {
        channel: "ocr",
        label: "OCR text capture",
        storage_kind: "database",
        count_query: "SELECT COUNT(*) FROM ocr_results",
        last_query: "SELECT MAX(timestamp) FROM ocr_results",
    },
    CaptureChannelMeta {
        channel: "screen_frames",
        label: "Screen keyframes / evidence",
        storage_kind: "files + database",
        count_query: "SELECT COUNT(*) FROM frames",
        last_query: "SELECT MAX(timestamp) FROM frames",
    },
];

fn file_size(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn directory_size(path: &Path) -> u64 {
    fn walk(path: &Path) -> io::Result<u64> {
        if !path.exists() {
            return Ok(0);
        }

        let metadata = fs::metadata(path)?;
        if metadata.is_file() {
            return Ok(metadata.len());
        }

        let mut total = 0;
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            total += walk(&entry.path())?;
        }
        Ok(total)
    }

    walk(path).unwrap_or(0)
}

fn remove_file_if_exists(path: &Path) {
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}

fn remove_empty_parent_dirs(path: &Path, stop_at: &Path) {
    let mut current = path.parent();

    while let Some(dir) = current {
        if dir == stop_at {
            break;
        }

        let is_empty = fs::read_dir(dir)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);

        if !is_empty {
            break;
        }

        let _ = fs::remove_dir(dir);
        current = dir.parent();
    }
}

async fn ensure_capture_stopped(state: &AppState) -> Result<(), String> {
    if state.desktop_capture_runtime.read().await.is_active {
        Err("Stop capture before deleting data.".to_string())
    } else {
        Ok(())
    }
}

fn actual_recordings_path(state: &AppState) -> Result<PathBuf, String> {
    if let Some(storage) = state.storage.as_ref() {
        return Ok(storage.base_path());
    }

    let platform = get_platform();
    Ok(platform
        .get_data_directory()
        .map_err(|e| format!("Failed to resolve data directory: {}", e))?
        .join("recordings"))
}

#[cfg(target_family = "unix")]
fn get_disk_space_for_path(path: &Path) -> Result<(u64, u64), String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "Failed to prepare path for disk inspection.".to_string())?;
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::statvfs(c_path.as_ptr(), &mut stats) };

    if result != 0 {
        return Err(format!(
            "Failed to inspect disk space: {}",
            std::io::Error::last_os_error()
        ));
    }

    let block_size = stats.f_frsize as u64;
    let total_bytes = stats.f_blocks as u64 * block_size;
    let free_bytes = stats.f_bavail as u64 * block_size;

    Ok((total_bytes, free_bytes))
}

#[cfg(not(target_family = "unix"))]
fn get_disk_space_for_path(_path: &Path) -> Result<(u64, u64), String> {
    Err("Disk space inspection is not implemented on this platform.".to_string())
}

async fn collect_capture_data_overview(state: &AppState) -> Result<CaptureDataOverviewDto, String> {
    let database_path =
        Database::database_path().map_err(|e| format!("Failed to resolve database path: {}", e))?;
    let configured_storage_path = state
        .config
        .lock()
        .map_err(|e| format!("Failed to lock config: {}", e))?
        .storage_path
        .clone();
    let configured_storage_path_string = configured_storage_path.to_string_lossy().to_string();
    let actual_recordings_path = actual_recordings_path(state)?;
    let database_size_bytes = file_size(&database_path);
    let recordings_size_bytes = directory_size(&actual_recordings_path);
    let total_size_bytes = database_size_bytes + recordings_size_bytes;
    let disk_probe_path = if actual_recordings_path.exists() {
        actual_recordings_path.as_path()
    } else {
        database_path.parent().unwrap_or_else(|| Path::new("/"))
    };
    let (disk_total_bytes, disk_free_bytes) = get_disk_space_for_path(disk_probe_path)?;
    let disk_used_bytes = disk_total_bytes.saturating_sub(disk_free_bytes);
    let source_percent_of_disk = if disk_total_bytes > 0 {
        total_size_bytes as f32 / disk_total_bytes as f32 * 100.0
    } else {
        0.0
    };
    let source_percent_of_free_space = if disk_free_bytes > 0 {
        total_size_bytes as f32 / disk_free_bytes as f32 * 100.0
    } else {
        0.0
    };
    let disk_health = if disk_free_bytes <= 10 * 1024 * 1024 * 1024 {
        "critical".to_string()
    } else if disk_free_bytes <= 25 * 1024 * 1024 * 1024 {
        "warning".to_string()
    } else {
        "healthy".to_string()
    };
    let disk_warning = if disk_health == "critical" {
        Some("Disk space is critically low. Keep SOURCE lean and delete unused capture data quickly.".to_string())
    } else if disk_health == "warning" {
        Some("Disk space is getting tight. Watch SOURCE storage growth before longer capture sessions.".to_string())
    } else {
        None
    };

    let mut channels = Vec::new();
    for channel in CAPTURE_CHANNELS {
        let row_count: i64 = sqlx::query_scalar(channel.count_query)
            .fetch_one(state.db.pool())
            .await
            .map_err(|e| format!("Failed to inspect {} rows: {}", channel.channel, e))?;
        let last_event_time: Option<i64> = sqlx::query_scalar(channel.last_query)
            .fetch_one(state.db.pool())
            .await
            .map_err(|e| format!("Failed to inspect {} timestamp: {}", channel.channel, e))?;
        channels.push(CaptureDataChannelUsageDto {
            channel: channel.channel.to_string(),
            label: channel.label.to_string(),
            storage_kind: channel.storage_kind.to_string(),
            row_count,
            disk_bytes: if channel.channel == "screen_frames" {
                recordings_size_bytes
            } else {
                0
            },
            last_event_time,
        });
    }

    let mut notes = vec![
        "Most capture channels currently store rows inside the local SOURCE SQLite database.".to_string(),
        "Screen keyframes and encoded evidence segments also write files into the recordings directory.".to_string(),
        "Delete actions are disabled while capture is running so SOURCE does not remove live data out from under an active session.".to_string(),
    ];

    if configured_storage_path != actual_recordings_path {
        notes.push(
            "The configured storage path and the runtime recordings path do not currently match, so the runtime path shown here is the source of truth for captured evidence files."
                .to_string(),
        );
    }

    Ok(CaptureDataOverviewDto {
        database_path: database_path.to_string_lossy().to_string(),
        configured_storage_path: configured_storage_path_string,
        actual_recordings_path: actual_recordings_path.to_string_lossy().to_string(),
        database_size_bytes,
        recordings_size_bytes,
        total_size_bytes,
        disk_total_bytes,
        disk_free_bytes,
        disk_used_bytes,
        source_percent_of_disk,
        source_percent_of_free_space,
        disk_health,
        disk_warning,
        channels,
        notes,
    })
}

async fn clear_screen_evidence_channel(state: &AppState) -> Result<(), String> {
    let recordings_root = actual_recordings_path(state)?;
    let frame_paths: Vec<String> = sqlx::query_scalar("SELECT file_path FROM frames")
        .fetch_all(state.db.pool())
        .await
        .map_err(|e| format!("Failed to list frame paths: {}", e))?;
    let segment_paths: Vec<String> = sqlx::query_scalar("SELECT file_path FROM video_segments")
        .fetch_all(state.db.pool())
        .await
        .map_err(|e| format!("Failed to list segment paths: {}", e))?;
    let base_layer_paths: Vec<Option<String>> =
        sqlx::query_scalar("SELECT base_layer_path FROM screen_recordings")
            .fetch_all(state.db.pool())
            .await
            .map_err(|e| format!("Failed to list base-layer paths: {}", e))?;
    let session_dirs: Vec<Option<String>> =
        sqlx::query_scalar("SELECT recording_path FROM sessions")
            .fetch_all(state.db.pool())
            .await
            .map_err(|e| format!("Failed to list recording paths: {}", e))?;

    for path in frame_paths {
        let file_path = PathBuf::from(&path);
        remove_file_if_exists(&file_path);
        remove_empty_parent_dirs(&file_path, &recordings_root);
    }
    for path in segment_paths {
        let file_path = PathBuf::from(&path);
        remove_file_if_exists(&file_path);
        remove_empty_parent_dirs(&file_path, &recordings_root);
    }
    for path in base_layer_paths.into_iter().flatten() {
        let file_path = PathBuf::from(&path);
        remove_file_if_exists(&file_path);
        remove_empty_parent_dirs(&file_path, &recordings_root);
    }

    sqlx::query("DELETE FROM screen_recordings")
        .execute(state.db.pool())
        .await
        .map_err(|e| format!("Failed to clear screen recordings: {}", e))?;
    sqlx::query("DELETE FROM video_segments")
        .execute(state.db.pool())
        .await
        .map_err(|e| format!("Failed to clear video segments: {}", e))?;
    sqlx::query("DELETE FROM frames")
        .execute(state.db.pool())
        .await
        .map_err(|e| format!("Failed to clear frames: {}", e))?;
    sqlx::query(
        "UPDATE sessions
         SET frame_count = 0,
             total_size_bytes = 0,
             segment_count = 0,
             total_motion_percentage = 0.0,
             recording_path = NULL,
             base_layer_path = NULL",
    )
    .execute(state.db.pool())
    .await
    .map_err(|e| format!("Failed to reset session evidence metadata: {}", e))?;

    for session_dir in session_dirs.into_iter().flatten() {
        let session_path = PathBuf::from(session_dir);
        if session_path.exists() {
            let frames_dir = session_path.join("frames");
            let segments_dir = session_path.join("segments");
            if frames_dir.exists() {
                let _ = fs::remove_dir_all(&frames_dir);
            }
            if segments_dir.exists() {
                let _ = fs::remove_dir_all(&segments_dir);
            }
            let _ = fs::remove_file(session_path.join("base_layer.png"));
            let is_empty = fs::read_dir(&session_path)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);
            if is_empty {
                let _ = fs::remove_dir(&session_path);
            }
        }
    }

    Ok(())
}

fn pretty_json(value: serde_json::Value) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

async fn persist_context_snapshot(
    db: Arc<Database>,
    session_id: Option<String>,
    frontmost: Option<AppInfo>,
    running_apps: Vec<AppInfo>,
    prev_frontmost_bundle_id: &mut Option<String>,
    prev_running_ids: &mut HashSet<String>,
) -> Result<(), String> {
    let timestamp = chrono::Utc::now().timestamp_millis();
    let visible_windows =
        context_timeline::app_infos_to_visible_windows(frontmost.as_ref(), &running_apps);

    context_timeline::insert_window_snapshot(
        &db,
        session_id.as_deref(),
        timestamp,
        frontmost.as_ref().map(|app| app.name.as_str()),
        frontmost.as_ref().map(|app| app.bundle_id.as_str()),
        &visible_windows,
        "desktop_sampler",
        0.68,
    )
    .await
    .map_err(|e| format!("Failed to save window snapshot: {}", e))?;

    if let Some(frontmost) = frontmost.as_ref() {
        let has_changed = prev_frontmost_bundle_id
            .as_ref()
            .map(|bundle_id| bundle_id != &frontmost.bundle_id)
            .unwrap_or(true);
        if has_changed {
            context_timeline::insert_context_event(
                &db,
                session_id.as_deref(),
                timestamp,
                "focus",
                "frontmost_changed",
                "desktop_sampler",
                0.92,
                Some(
                    serde_json::json!({
                        "title": format!("Focused {}", frontmost.name),
                        "subtitle": frontmost.bundle_id,
                        "app_name": frontmost.name,
                    })
                    .to_string(),
                ),
            )
            .await
            .map_err(|e| format!("Failed to save focus event: {}", e))?;
            *prev_frontmost_bundle_id = Some(frontmost.bundle_id.clone());
        }
    }

    let current_running_ids = running_apps
        .iter()
        .map(|app| app.bundle_id.clone())
        .collect::<HashSet<_>>();

    for launched in current_running_ids.difference(prev_running_ids) {
        if let Some(app) = running_apps.iter().find(|app| &app.bundle_id == launched) {
            let _ = context_timeline::insert_context_event(
                &db,
                session_id.as_deref(),
                timestamp,
                "system",
                "app_launch_detected",
                "desktop_sampler",
                0.6,
                Some(
                    serde_json::json!({
                        "title": format!("{} appeared", app.name),
                        "subtitle": "Running app set changed",
                        "app_name": app.name,
                    })
                    .to_string(),
                ),
            )
            .await;
        }
    }

    for bundle_id in prev_running_ids.difference(&current_running_ids) {
        let _ = context_timeline::insert_context_event(
            &db,
            session_id.as_deref(),
            timestamp,
            "system",
            "app_quit_detected",
            "desktop_sampler",
            0.45,
            Some(
                serde_json::json!({
                    "title": "Running app disappeared",
                    "subtitle": bundle_id,
                })
                .to_string(),
            ),
        )
        .await;
    }

    *prev_running_ids = current_running_ids;

    Ok(())
}

async fn spawn_desktop_sampler(
    db: Arc<Database>,
    os_activity_recorder: Option<Arc<OsActivityRecorder>>,
    config: Arc<Mutex<Config>>,
    runtime: Arc<RwLock<DesktopCaptureRuntime>>,
) {
    let mut prev_frontmost_bundle_id: Option<String> = None;
    let mut prev_running_ids = HashSet::new();

    loop {
        let (generation, active, session_id) = {
            let state = runtime.read().await;
            (
                state.sampler_generation,
                state.is_active,
                state.session_id.clone(),
            )
        };

        if !active {
            break;
        }

        let profile = config
            .lock()
            .map(|cfg| cfg.resource_profile.clone())
            .unwrap_or(ResourceProfile::Balanced);
        let capture_scene = config
            .lock()
            .map(|cfg| {
                cfg.capture_channels.system
                    || cfg.capture_channels.focus
                    || cfg.capture_channels.visible_windows
            })
            .unwrap_or(false);

        if capture_scene {
            if let Some(os_activity_recorder) = os_activity_recorder.as_ref() {
                let frontmost = os_activity_recorder.get_current_app().await.ok().flatten();
                let running_apps = os_activity_recorder
                    .get_running_apps()
                    .await
                    .unwrap_or_default();
                if let Err(error) = persist_context_snapshot(
                    db.clone(),
                    session_id.clone(),
                    frontmost,
                    running_apps,
                    &mut prev_frontmost_bundle_id,
                    &mut prev_running_ids,
                )
                .await
                {
                    let mut state = runtime.write().await;
                    state
                        .channel_errors
                        .insert("visible_windows".to_string(), error);
                }
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(interval_for_profile(
            &profile,
        )))
        .await;

        let still_same_generation = runtime.read().await.sampler_generation == generation;
        if !still_same_generation {
            break;
        }
    }
}

#[tauri::command]
async fn start_desktop_capture(
    display_id: Option<u32>,
    state: State<'_, AppState>,
) -> Result<DesktopCaptureStatusDto, String> {
    let mut runtime = state.desktop_capture_runtime.write().await;
    if runtime.is_active {
        drop(runtime);
        return build_desktop_capture_status(&state).await;
    }

    let config = state
        .config
        .lock()
        .map_err(|e| format!("Failed to lock config: {}", e))?
        .clone();
    let session_manager = state
        .session_manager
        .as_ref()
        .ok_or("Session manager not initialized")?;
    let session_id = session_manager
        .get_or_create_session()
        .await
        .map_err(|e| format!("Failed to create session: {}", e))?;
    let started_at = chrono::Utc::now().timestamp_millis();

    runtime.is_active = false;
    runtime.session_id = Some(session_id.clone());
    runtime.started_at = Some(started_at);
    runtime.display_id = display_id;
    runtime.display_name = None;
    runtime.warnings.clear();
    runtime.channel_errors.clear();
    runtime.sampler_generation += 1;
    let current_generation = runtime.sampler_generation;
    drop(runtime);

    let mut started_any_channel = false;

    if config.capture_channels.system
        || config.capture_channels.focus
        || config.capture_channels.visible_windows
    {
        if let Some(recorder) = state.os_activity_recorder.as_ref() {
            if let Err(error) = recorder.start_recording(session_id.clone()).await {
                let mut runtime = state.desktop_capture_runtime.write().await;
                runtime
                    .channel_errors
                    .insert("system".to_string(), error.to_string());
                runtime
                    .warnings
                    .push("OS activity capture could not start.".to_string());
            } else {
                started_any_channel = true;
            }
        }
    }

    if config.capture_channels.keyboard || config.capture_channels.mouse {
        if let Some(recorder) = state.input_recorder.as_ref() {
            if let Err(error) = recorder
                .start_recording_with_options(
                    session_id.clone(),
                    config.capture_channels.keyboard,
                    config.capture_channels.mouse,
                )
                .await
            {
                let mut runtime = state.desktop_capture_runtime.write().await;
                runtime
                    .channel_errors
                    .insert("input".to_string(), error.to_string());
                runtime.warnings.push(
                    "Input capture could not start with the current permissions.".to_string(),
                );
            } else {
                started_any_channel = true;
            }
        }
    }

    let ocr_requested = config.capture_channels.ocr && config.ocr_enabled;
    if ocr_requested && state.ocr_processor.is_none() {
        let mut runtime = state.desktop_capture_runtime.write().await;
        runtime.channel_errors.insert(
            "ocr".to_string(),
            "OCR engine is unavailable on this device right now.".to_string(),
        );
        runtime.warnings.push(
            "OCR capture is enabled, but the OCR engine could not be initialized.".to_string(),
        );
    }

    if let Some(recorder) = state.screen_recorder.as_ref() {
        recorder
            .configure_ocr_capture(
                ocr_requested && state.ocr_processor.is_some(),
                config.ocr_interval_seconds,
            )
            .await;
    }

    if config.capture_channels.screen_frames || ocr_requested {
        if let Some(recorder) = state.screen_recorder.as_ref() {
            match recorder.get_available_displays().await {
                Ok(displays) if displays.is_empty() => {
                    let mut runtime = state.desktop_capture_runtime.write().await;
                    runtime.channel_errors.insert(
                        if config.capture_channels.screen_frames {
                            "screen_frames".to_string()
                        } else {
                            "ocr".to_string()
                        },
                        "No capturable displays are available.".to_string(),
                    );
                    runtime
                        .warnings
                        .push(
                            if config.capture_channels.screen_frames {
                                "Screen evidence capture could not start because no capturable displays are available.".to_string()
                            } else {
                                "OCR capture could not start because no capturable displays are available.".to_string()
                            },
                        );
                }
                Ok(displays) => {
                    let fallback_display = displays
                        .iter()
                        .find(|display| display.is_primary)
                        .or_else(|| displays.first())
                        .cloned();
                    let resolved_display = match (display_id, fallback_display) {
                        (Some(requested_id), _)
                            if displays.iter().any(|display| display.id == requested_id) =>
                        {
                            displays
                                .iter()
                                .find(|display| display.id == requested_id)
                                .cloned()
                        }
                        (Some(_), Some(primary_display)) => {
                            let mut runtime = state.desktop_capture_runtime.write().await;
                            runtime.warnings.push(format!(
                                "The previously selected display is no longer available, so SOURCE switched to {}.",
                                primary_display.name
                            ));
                            Some(primary_display)
                        }
                        (None, Some(primary_display)) => {
                            let mut runtime = state.desktop_capture_runtime.write().await;
                            runtime.warnings.push(format!(
                                "No display was selected, so SOURCE defaulted to {}.",
                                primary_display.name
                            ));
                            Some(primary_display)
                        }
                        _ => None,
                    };

                    if let Some(display) = resolved_display {
                        if let Err(error) = recorder.start_recording(display.id).await {
                            let mut runtime = state.desktop_capture_runtime.write().await;
                            runtime.channel_errors.insert(
                                if config.capture_channels.screen_frames {
                                    "screen_frames".to_string()
                                } else {
                                    "ocr".to_string()
                                },
                                error.to_string(),
                            );
                            runtime.warnings.push(if config.capture_channels.screen_frames {
                                format!(
                                    "Screen evidence capture could not start because display sampling failed: {}.",
                                    error
                                )
                            } else {
                                format!(
                                    "OCR capture could not start because display sampling failed: {}.",
                                    error
                                )
                            });
                        } else if let Ok(status) = recorder.get_status().await {
                            let mut runtime = state.desktop_capture_runtime.write().await;
                            runtime.display_id = Some(display.id);
                            runtime.display_name =
                                status.display_name.or(Some(display.name.clone()));
                            started_any_channel = true;
                        }
                    } else {
                        let mut runtime = state.desktop_capture_runtime.write().await;
                        runtime.warnings.push(
                            "No display was selected, so display-based channels are currently disabled.".to_string(),
                        );
                    }
                }
                Err(error) => {
                    let mut runtime = state.desktop_capture_runtime.write().await;
                    runtime.channel_errors.insert(
                        if config.capture_channels.screen_frames {
                            "screen_frames".to_string()
                        } else {
                            "ocr".to_string()
                        },
                        error.to_string(),
                    );
                    runtime
                        .warnings
                        .push(if config.capture_channels.screen_frames {
                            format!(
                                "Screen evidence capture could not inspect available displays: {}.",
                                error
                            )
                        } else {
                            format!(
                                "OCR capture could not inspect available displays: {}.",
                                error
                            )
                        });
                }
            }
        }
    }

    {
        let mut runtime = state.desktop_capture_runtime.write().await;
        runtime.is_active = started_any_channel;
        if !started_any_channel {
            runtime.session_id = None;
            runtime.started_at = None;
            runtime.display_id = None;
            runtime.display_name = None;
            if runtime.warnings.is_empty() {
                runtime.warnings.push(
                    "No capture channels could start with the current configuration.".to_string(),
                );
            }
        }
    }

    if !started_any_channel {
        let _ = session_manager.end_current_session().await;
        return build_desktop_capture_status(&state).await;
    }

    context_timeline::insert_context_event(
        &state.db,
        Some(&session_id),
        started_at,
        "system",
        "capture_started",
        "desktop_capture",
        1.0,
        Some(
            serde_json::json!({
                "title": "Desktop capture started",
                "subtitle": "SOURCE is now sampling desktop context",
            })
            .to_string(),
        ),
    )
    .await
    .map_err(|e| format!("Failed to persist capture start event: {}", e))?;

    let db = state.db.clone();
    let os_activity = state.os_activity_recorder.clone();
    let config_handle = state.config.clone();
    let runtime_handle = state.desktop_capture_runtime.clone();
    tauri::async_runtime::spawn(async move {
        {
            let mut state = runtime_handle.write().await;
            state.sampler_generation = current_generation;
        }
        spawn_desktop_sampler(db, os_activity, config_handle, runtime_handle).await;
    });

    build_desktop_capture_status(&state).await
}

#[tauri::command]
async fn stop_desktop_capture(
    state: State<'_, AppState>,
) -> Result<DesktopCaptureStatusDto, String> {
    let session_id = state
        .desktop_capture_runtime
        .read()
        .await
        .session_id
        .clone();

    if let Some(recorder) = state.input_recorder.as_ref() {
        let _ = recorder.stop_recording().await;
    }
    if let Some(recorder) = state.os_activity_recorder.as_ref() {
        let _ = recorder.stop_recording().await;
    }
    if let Some(recorder) = state.screen_recorder.as_ref() {
        let _ = recorder.stop_recording().await;
    }
    if let Some(manager) = state.session_manager.as_ref() {
        let _ = manager.end_current_session().await;
    }

    if let Some(session_id) = session_id.as_ref() {
        let _ = context_timeline::insert_context_event(
            &state.db,
            Some(session_id),
            chrono::Utc::now().timestamp_millis(),
            "system",
            "capture_stopped",
            "desktop_capture",
            1.0,
            Some(
                serde_json::json!({
                    "title": "Desktop capture stopped",
                    "subtitle": "SOURCE ended the current multi-channel capture session",
                })
                .to_string(),
            ),
        )
        .await;
    }

    {
        let mut runtime = state.desktop_capture_runtime.write().await;
        runtime.is_active = false;
        runtime.session_id = None;
        runtime.started_at = None;
        runtime.sampler_generation += 1;
    }

    build_desktop_capture_status(&state).await
}

#[tauri::command]
async fn get_desktop_capture_status(
    state: State<'_, AppState>,
) -> Result<DesktopCaptureStatusDto, String> {
    build_desktop_capture_status(&state).await
}

#[tauri::command]
async fn get_channel_statuses(state: State<'_, AppState>) -> Result<Vec<ChannelStatusDto>, String> {
    let config = state
        .config
        .lock()
        .map_err(|e| format!("Failed to lock config: {}", e))?
        .clone();
    let runtime = state.desktop_capture_runtime.read().await.clone();

    let channels = vec![
        ("system", config.capture_channels.system, "context_events", "SELECT MAX(timestamp) FROM context_events WHERE channel = 'system'", "SELECT COUNT(*) FROM context_events WHERE channel = 'system' AND timestamp >= strftime('%s','now') * 1000 - 3600000"),
        ("focus", config.capture_channels.focus, "context_events", "SELECT MAX(timestamp) FROM context_events WHERE channel = 'focus'", "SELECT COUNT(*) FROM context_events WHERE channel = 'focus' AND timestamp >= strftime('%s','now') * 1000 - 3600000"),
        ("visible_windows", config.capture_channels.visible_windows, "window_snapshots", "SELECT MAX(timestamp) FROM window_snapshots", "SELECT COUNT(*) FROM window_snapshots WHERE timestamp >= strftime('%s','now') * 1000 - 3600000"),
        ("keyboard", config.capture_channels.keyboard, "keyboard_events", "SELECT MAX(timestamp) FROM keyboard_events", "SELECT COUNT(*) FROM keyboard_events WHERE timestamp >= strftime('%s','now') * 1000 - 3600000"),
        ("mouse", config.capture_channels.mouse, "mouse_events", "SELECT MAX(timestamp) FROM mouse_events", "SELECT COUNT(*) FROM mouse_events WHERE timestamp >= strftime('%s','now') * 1000 - 3600000"),
        ("ocr", config.capture_channels.ocr, "ocr_results", "SELECT MAX(timestamp) FROM ocr_results", "SELECT COUNT(*) FROM ocr_results WHERE timestamp >= strftime('%s','now') * 1000 - 3600000"),
        ("screen_frames", config.capture_channels.screen_frames, "frames", "SELECT MAX(timestamp) FROM frames", "SELECT COUNT(*) FROM frames WHERE timestamp >= strftime('%s','now') * 1000 - 3600000"),
    ];

    let mut statuses = Vec::new();
    for (channel, enabled, _table, last_query, count_query) in channels {
        let permission_state = channel_permission_state(&state.consent_manager, channel).await;
        let last_event_time =
            context_timeline::get_last_event_time_for_table(&state.db, last_query)
                .await
                .map_err(|e| format!("Failed to inspect channel status: {}", e))?;
        let sample_count = context_timeline::get_count_for_query(&state.db, count_query)
            .await
            .map_err(|e| format!("Failed to inspect channel count: {}", e))?;
        let health = if !enabled {
            "off"
        } else if runtime.channel_errors.contains_key(channel) {
            "degraded"
        } else if permission_state == "missing" {
            "degraded"
        } else if last_event_time.is_some() {
            "healthy"
        } else if runtime.is_active {
            "warming_up"
        } else {
            "idle"
        };
        statuses.push(ChannelStatusDto {
            channel: channel.to_string(),
            enabled,
            health: health.to_string(),
            permission_state,
            last_event_time,
            sample_count,
            throughput_per_minute: sample_count as f32 / 60.0,
            last_error: runtime.channel_errors.get(channel).cloned(),
            supports_solo_test: !matches!(channel, "ocr"),
            details: match channel {
                "visible_windows" => "Best-effort running-app snapshots, not a full historical window server feed.".to_string(),
                "ocr" => "OCR review stays available when text exists, but live OCR ingestion is still intentionally degradable in v1.".to_string(),
                _ => "Ready for independent channel validation.".to_string(),
            },
        });
    }

    Ok(statuses)
}

#[tauri::command]
async fn get_capture_data_overview(
    state: State<'_, AppState>,
) -> Result<CaptureDataOverviewDto, String> {
    collect_capture_data_overview(&state).await
}

#[tauri::command]
async fn reveal_capture_data_target(
    target: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let target_path = match target.as_str() {
        "database" => Database::database_path()
            .map_err(|e| format!("Failed to resolve database path: {}", e))?,
        "recordings" => actual_recordings_path(&state)?,
        "configured_storage" => PathBuf::from(
            state
                .config
                .lock()
                .map_err(|e| format!("Failed to lock config: {}", e))?
                .storage_path
                .clone(),
        ),
        _ => return Err(format!("Unknown reveal target: {}", target)),
    };

    let mut command = ProcessCommand::new("open");
    if target == "database" {
        command.arg("-R").arg(&target_path);
    } else {
        command.arg(&target_path);
    }

    command
        .status()
        .map_err(|e| format!("Failed to reveal path: {}", e))
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err("Reveal command exited unsuccessfully.".to_string())
            }
        })
}

#[tauri::command]
async fn delete_capture_data(
    channel: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    ensure_capture_stopped(&state).await?;

    match channel.as_deref() {
        Some("system") => {
            sqlx::query("DELETE FROM context_events WHERE channel = 'system'")
                .execute(state.db.pool())
                .await
                .map_err(|e| format!("Failed to delete system events: {}", e))?;
        }
        Some("focus") => {
            sqlx::query("DELETE FROM context_events WHERE channel = 'focus'")
                .execute(state.db.pool())
                .await
                .map_err(|e| format!("Failed to delete focus events: {}", e))?;
        }
        Some("visible_windows") => {
            sqlx::query("DELETE FROM window_snapshots")
                .execute(state.db.pool())
                .await
                .map_err(|e| format!("Failed to delete visible window snapshots: {}", e))?;
        }
        Some("keyboard") => {
            sqlx::query("DELETE FROM keyboard_events")
                .execute(state.db.pool())
                .await
                .map_err(|e| format!("Failed to delete keyboard events: {}", e))?;
        }
        Some("mouse") => {
            sqlx::query("DELETE FROM mouse_events")
                .execute(state.db.pool())
                .await
                .map_err(|e| format!("Failed to delete mouse events: {}", e))?;
        }
        Some("ocr") => {
            sqlx::query("DELETE FROM ocr_results")
                .execute(state.db.pool())
                .await
                .map_err(|e| format!("Failed to delete OCR results: {}", e))?;
            ocr_agent_context::delete_all_derived(&state.db)
                .await
                .map_err(|e| format!("Failed to delete derived OCR data: {}", e))?;
        }
        Some("screen_frames") => {
            clear_screen_evidence_channel(&state).await?;
        }
        None => {
            let recordings_root = actual_recordings_path(&state)?;
            clear_screen_evidence_channel(&state).await?;
            sqlx::query("DELETE FROM ocr_results")
                .execute(state.db.pool())
                .await
                .map_err(|e| format!("Failed to delete OCR results: {}", e))?;
            ocr_agent_context::delete_all_derived(&state.db)
                .await
                .map_err(|e| format!("Failed to delete derived OCR data: {}", e))?;
            sqlx::query("DELETE FROM keyboard_events")
                .execute(state.db.pool())
                .await
                .map_err(|e| format!("Failed to delete keyboard events: {}", e))?;
            sqlx::query("DELETE FROM mouse_events")
                .execute(state.db.pool())
                .await
                .map_err(|e| format!("Failed to delete mouse events: {}", e))?;
            sqlx::query("DELETE FROM window_snapshots")
                .execute(state.db.pool())
                .await
                .map_err(|e| format!("Failed to delete visible window snapshots: {}", e))?;
            sqlx::query("DELETE FROM context_events")
                .execute(state.db.pool())
                .await
                .map_err(|e| format!("Failed to delete context events: {}", e))?;
            sqlx::query("DELETE FROM sessions")
                .execute(state.db.pool())
                .await
                .map_err(|e| format!("Failed to delete sessions: {}", e))?;
            if recordings_root.exists() {
                let _ = fs::remove_dir_all(&recordings_root);
            }
            fs::create_dir_all(&recordings_root)
                .map_err(|e| format!("Failed to recreate recordings folder: {}", e))?;
        }
        Some(other) => {
            return Err(format!("Unknown channel: {}", other));
        }
    }

    sqlx::query("VACUUM")
        .execute(state.db.pool())
        .await
        .map_err(|e| format!("Failed to compact database: {}", e))?;

    Ok(())
}

#[tauri::command]
async fn get_capture_channel_preview(
    channel: String,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> Result<CapturePreviewDto, String> {
    let limit = limit.unwrap_or(20).clamp(1, 100);
    let label = CAPTURE_CHANNELS
        .iter()
        .find(|meta| meta.channel == channel)
        .map(|meta| meta.label.to_string())
        .unwrap_or_else(|| channel.clone());

    let rows = match channel.as_str() {
        "system" | "focus" => {
            let db_rows = sqlx::query(
                "SELECT timestamp, event_type, source, confidence, payload_json
                 FROM context_events
                 WHERE channel = ?
                 ORDER BY timestamp DESC
                 LIMIT ?",
            )
            .bind(&channel)
            .bind(limit)
            .fetch_all(state.db.pool())
            .await
            .map_err(|e| format!("Failed to load {} preview: {}", channel, e))?;

            db_rows
                .into_iter()
                .map(|row| {
                    let timestamp = row.get::<i64, _>("timestamp");
                    let event_type = row.get::<String, _>("event_type");
                    let source = row.get::<String, _>("source");
                    let confidence = row.get::<f64, _>("confidence");
                    let payload_json = row.get::<Option<String>, _>("payload_json");
                    let raw = serde_json::json!({
                        "timestamp": timestamp,
                        "event_type": event_type,
                        "source": source,
                        "confidence": confidence,
                        "payload_json": payload_json,
                    });
                    CapturePreviewRowDto {
                        timestamp: Some(timestamp),
                        summary: format!("{event_type} via {source}"),
                        raw_json: pretty_json(raw),
                    }
                })
                .collect()
        }
        "visible_windows" => {
            let db_rows = sqlx::query(
                "SELECT timestamp, frontmost_app_name, frontmost_bundle_id, visible_windows_json, confidence
                 FROM window_snapshots
                 ORDER BY timestamp DESC
                 LIMIT ?",
            )
            .bind(limit)
            .fetch_all(state.db.pool())
            .await
            .map_err(|e| format!("Failed to load visible windows preview: {}", e))?;

            db_rows
                .into_iter()
                .map(|row| {
                    let timestamp = row.get::<i64, _>("timestamp");
                    let app_name = row.get::<Option<String>, _>("frontmost_app_name");
                    let bundle_id = row.get::<Option<String>, _>("frontmost_bundle_id");
                    let visible_windows_json = row.get::<String, _>("visible_windows_json");
                    let confidence = row.get::<f64, _>("confidence");
                    let raw = serde_json::json!({
                        "timestamp": timestamp,
                        "frontmost_app_name": app_name,
                        "frontmost_bundle_id": bundle_id,
                        "confidence": confidence,
                        "visible_windows": serde_json::from_str::<serde_json::Value>(&visible_windows_json).unwrap_or(serde_json::Value::String(visible_windows_json)),
                    });
                    CapturePreviewRowDto {
                        timestamp: Some(timestamp),
                        summary: format!("Frontmost app: {}", app_name.clone().unwrap_or_else(|| "Unknown".to_string())),
                        raw_json: pretty_json(raw),
                    }
                })
                .collect()
        }
        "keyboard" => {
            let db_rows = sqlx::query(
                "SELECT timestamp, event_type, key_code, key_char, modifiers, app_name, window_title, process_id, ui_element
                 FROM keyboard_events
                 ORDER BY timestamp DESC
                 LIMIT ?",
            )
            .bind(limit)
            .fetch_all(state.db.pool())
            .await
            .map_err(|e| format!("Failed to load keyboard preview: {}", e))?;

            db_rows
                .into_iter()
                .map(|row| {
                    let timestamp = row.get::<i64, _>("timestamp");
                    let event_type = row.get::<String, _>("event_type");
                    let key_code = row.get::<i64, _>("key_code");
                    let key_char = row.get::<Option<String>, _>("key_char");
                    let modifiers = row.get::<String, _>("modifiers");
                    let app_name = row.get::<String, _>("app_name");
                    let window_title = row.get::<String, _>("window_title");
                    let process_id = row.get::<i64, _>("process_id");
                    let ui_element = row.get::<Option<String>, _>("ui_element");
                    let raw = serde_json::json!({
                        "timestamp": timestamp,
                        "event_type": event_type,
                        "key_code": key_code,
                        "key_char": key_char,
                        "modifiers": serde_json::from_str::<serde_json::Value>(&modifiers).unwrap_or(serde_json::Value::String(modifiers)),
                        "app_name": app_name,
                        "window_title": window_title,
                        "process_id": process_id,
                        "ui_element": ui_element.and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok()).unwrap_or(serde_json::Value::Null),
                    });
                    CapturePreviewRowDto {
                        timestamp: Some(timestamp),
                        summary: format!("{event_type} in {app_name}"),
                        raw_json: pretty_json(raw),
                    }
                })
                .collect()
        }
        "mouse" => {
            let db_rows = sqlx::query(
                "SELECT timestamp, event_type, position_x, position_y, app_name, window_title, process_id, ui_element
                 FROM mouse_events
                 ORDER BY timestamp DESC
                 LIMIT ?",
            )
            .bind(limit)
            .fetch_all(state.db.pool())
            .await
            .map_err(|e| format!("Failed to load mouse preview: {}", e))?;

            db_rows
                .into_iter()
                .map(|row| {
                    let timestamp = row.get::<i64, _>("timestamp");
                    let event_type = row.get::<String, _>("event_type");
                    let x = row.get::<i64, _>("position_x");
                    let y = row.get::<i64, _>("position_y");
                    let app_name = row.get::<String, _>("app_name");
                    let window_title = row.get::<String, _>("window_title");
                    let process_id = row.get::<i64, _>("process_id");
                    let ui_element = row.get::<Option<String>, _>("ui_element");
                    let raw = serde_json::json!({
                        "timestamp": timestamp,
                        "event_type": event_type,
                        "position_x": x,
                        "position_y": y,
                        "app_name": app_name,
                        "window_title": window_title,
                        "process_id": process_id,
                        "ui_element": ui_element.and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok()).unwrap_or(serde_json::Value::Null),
                    });
                    CapturePreviewRowDto {
                        timestamp: Some(timestamp),
                        summary: format!("{event_type} at ({x}, {y}) in {app_name}"),
                        raw_json: pretty_json(raw),
                    }
                })
                .collect()
        }
        "ocr" => {
            let db_rows = sqlx::query(
                "SELECT timestamp, text, confidence, frame_path, bounding_box, language, processing_time_ms
                 FROM ocr_results
                 ORDER BY timestamp DESC
                 LIMIT ?",
            )
            .bind(limit)
            .fetch_all(state.db.pool())
            .await
            .map_err(|e| format!("Failed to load OCR preview: {}", e))?;

            db_rows
                .into_iter()
                .map(|row| {
                    let timestamp = row.get::<i64, _>("timestamp");
                    let text = row.get::<String, _>("text");
                    let confidence = row.get::<f64, _>("confidence");
                    let frame_path = row.get::<Option<String>, _>("frame_path");
                    let bounding_box = row.get::<String, _>("bounding_box");
                    let language = row.get::<String, _>("language");
                    let processing_time_ms = row.get::<Option<i64>, _>("processing_time_ms");
                    let preview = text.chars().take(90).collect::<String>();
                    let raw = serde_json::json!({
                        "timestamp": timestamp,
                        "text": text,
                        "confidence": confidence,
                        "frame_path": frame_path,
                        "bounding_box": serde_json::from_str::<serde_json::Value>(&bounding_box).unwrap_or(serde_json::Value::String(bounding_box)),
                        "language": language,
                        "processing_time_ms": processing_time_ms,
                    });
                    CapturePreviewRowDto {
                        timestamp: Some(timestamp),
                        summary: preview,
                        raw_json: pretty_json(raw),
                    }
                })
                .collect()
        }
        "screen_frames" => {
            let db_rows = sqlx::query(
                "SELECT timestamp, file_path, width, height
                 FROM frames
                 ORDER BY timestamp DESC
                 LIMIT ?",
            )
            .bind(limit)
            .fetch_all(state.db.pool())
            .await
            .map_err(|e| format!("Failed to load frame preview: {}", e))?;

            db_rows
                .into_iter()
                .map(|row| {
                    let timestamp = row.get::<i64, _>("timestamp");
                    let file_path = row.get::<String, _>("file_path");
                    let width = row.get::<i64, _>("width");
                    let height = row.get::<i64, _>("height");
                    let raw = serde_json::json!({
                        "timestamp": timestamp,
                        "file_path": file_path,
                        "width": width,
                        "height": height,
                    });
                    CapturePreviewRowDto {
                        timestamp: Some(timestamp),
                        summary: format!("{width}×{height} frame"),
                        raw_json: pretty_json(raw),
                    }
                })
                .collect()
        }
        other => return Err(format!("Unknown channel: {}", other)),
    };

    Ok(CapturePreviewDto {
        channel,
        label,
        rows,
    })
}

#[tauri::command]
async fn get_context_timeline(
    start_timestamp: i64,
    end_timestamp: i64,
    state: State<'_, AppState>,
) -> Result<DesktopContextTimelineData, String> {
    context_timeline::build_context_timeline(&state.db, start_timestamp, end_timestamp)
        .await
        .map_err(|e| format!("Failed to build context timeline: {}", e))
}

#[tauri::command]
async fn get_context_inspector(
    timestamp: i64,
    state: State<'_, AppState>,
) -> Result<ContextInspectorDto, String> {
    context_timeline::get_context_inspector(&state.db, timestamp)
        .await
        .map_err(|e| format!("Failed to build context inspector: {}", e))
}

#[tauri::command]
async fn get_context_slice_detail(
    slice_id: String,
    rail_id: String,
    start_timestamp: i64,
    end_timestamp: i64,
    state: State<'_, AppState>,
) -> Result<ContextSliceDetailDto, String> {
    context_timeline::get_context_slice_detail(
        &state.db,
        &slice_id,
        &rail_id,
        start_timestamp,
        end_timestamp,
    )
    .await
    .map_err(|e| format!("Failed to build context slice detail: {}", e))
}

#[tauri::command]
async fn get_scene_snapshot(
    scene_id: String,
    state: State<'_, AppState>,
) -> Result<Option<AgentSceneSnapshotDto>, String> {
    ocr_agent_context::get_scene_snapshot(&state.db, &scene_id)
        .await
        .map_err(|e| format!("Failed to get OCR scene snapshot: {}", e))
}

#[tauri::command]
async fn get_scene_snapshots(
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AgentSceneSnapshotDto>, String> {
    ocr_agent_context::get_scene_snapshots(&state.db, start_timestamp, end_timestamp, app_filter)
        .await
        .map_err(|e| format!("Failed to get OCR scene snapshots: {}", e))
}

#[tauri::command]
async fn get_text_spans(
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AgentTextSpanDto>, String> {
    ocr_agent_context::get_text_spans(&state.db, start_timestamp, end_timestamp, app_filter)
        .await
        .map_err(|e| format!("Failed to get OCR text spans: {}", e))
}

#[tauri::command]
async fn get_context_entities(
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
    entity_type: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AgentContextEntityDto>, String> {
    ocr_agent_context::get_context_entities(
        &state.db,
        start_timestamp,
        end_timestamp,
        app_filter,
        entity_type,
    )
    .await
    .map_err(|e| format!("Failed to get OCR context entities: {}", e))
}

#[tauri::command]
async fn search_agent_context(
    query: String,
    start_timestamp: Option<i64>,
    end_timestamp: Option<i64>,
    app_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AgentContextSearchResultDto>, String> {
    ocr_agent_context::search_agent_context(
        &state.db,
        &query,
        start_timestamp,
        end_timestamp,
        app_filter,
    )
    .await
    .map_err(|e| format!("Failed to search OCR agent context: {}", e))
}

#[tauri::command]
async fn get_activity_episode(
    timestamp: i64,
    state: State<'_, AppState>,
) -> Result<ActivityEpisodeDto, String> {
    ocr_agent_context::get_activity_episode(&state.db, timestamp)
        .await
        .map_err(|e| format!("Failed to get OCR activity episode: {}", e))
}

#[tauri::command]
async fn get_ocr_agent_summary(
    start_timestamp: i64,
    end_timestamp: i64,
    state: State<'_, AppState>,
) -> Result<OcrAgentSummaryDto, String> {
    ocr_agent_context::get_ocr_agent_summary(&state.db, start_timestamp, end_timestamp)
        .await
        .map_err(|e| format!("Failed to get OCR agent summary: {}", e))
}

#[tauri::command]
async fn get_app_usage_overview(
    start_timestamp: i64,
    end_timestamp: i64,
    state: State<'_, AppState>,
) -> Result<AppUsageOverviewDto, String> {
    context_timeline::get_app_usage_overview(&state.db, start_timestamp, end_timestamp)
        .await
        .map_err(|e| format!("Failed to get app usage overview: {}", e))
}

#[tauri::command]
async fn get_pii_review(
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
    entity_type_filter: Option<String>,
    confidence_threshold: Option<f32>,
    state: State<'_, AppState>,
) -> Result<Vec<PiiEntityDto>, String> {
    context_timeline::get_pii_review(
        &state.db,
        start_timestamp,
        end_timestamp,
        app_filter,
        entity_type_filter,
        confidence_threshold,
    )
    .await
    .map_err(|e| format!("Failed to get PII review: {}", e))
}

#[tauri::command]
async fn get_ocr_review(
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
    query: Option<String>,
    pii_only: bool,
    state: State<'_, AppState>,
) -> Result<Vec<OcrReviewItemDto>, String> {
    context_timeline::get_ocr_review(
        &state.db,
        start_timestamp,
        end_timestamp,
        app_filter,
        query,
        pii_only,
    )
    .await
    .map_err(|e| format!("Failed to get OCR review: {}", e))
}

// OS monitoring commands
#[tauri::command]
async fn start_os_monitoring(session_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let recorder = state
        .os_activity_recorder
        .as_ref()
        .ok_or("OS activity recorder not initialized")?;

    recorder
        .start_recording(session_id)
        .await
        .map_err(|e| format!("Failed to start OS monitoring: {}", e))
}

#[tauri::command]
async fn stop_os_monitoring(state: State<'_, AppState>) -> Result<(), String> {
    let recorder = state
        .os_activity_recorder
        .as_ref()
        .ok_or("OS activity recorder not initialized")?;

    recorder
        .stop_recording()
        .await
        .map_err(|e| format!("Failed to stop OS monitoring: {}", e))
}

#[tauri::command]
async fn get_app_usage_stats(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<AppUsageStats>, String> {
    let recorder = state
        .os_activity_recorder
        .as_ref()
        .ok_or("OS activity recorder not initialized")?;

    recorder
        .get_app_usage_stats(session_id)
        .await
        .map_err(|e| format!("Failed to get app usage stats: {}", e))
}

#[tauri::command]
async fn get_running_applications(state: State<'_, AppState>) -> Result<Vec<AppInfo>, String> {
    let recorder = state
        .os_activity_recorder
        .as_ref()
        .ok_or("OS activity recorder not initialized")?;

    recorder
        .get_running_apps()
        .await
        .map_err(|e| format!("Failed to get running apps: {}", e))
}

#[tauri::command]
async fn get_current_application(state: State<'_, AppState>) -> Result<Option<AppInfo>, String> {
    let recorder = state
        .os_activity_recorder
        .as_ref()
        .ok_or("OS activity recorder not initialized")?;

    recorder
        .get_current_app()
        .await
        .map_err(|e| format!("Failed to get current app: {}", e))
}

// Session management commands
#[tauri::command]
async fn get_current_session(state: State<'_, AppState>) -> Result<Option<Session>, String> {
    let manager = state
        .session_manager
        .as_ref()
        .ok_or("Session manager not initialized")?;

    manager
        .get_current_session()
        .await
        .map_err(|e| format!("Failed to get current session: {}", e))
}

#[tauri::command]
async fn get_session_history(
    start: i64,
    end: i64,
    state: State<'_, AppState>,
) -> Result<Vec<Session>, String> {
    let manager = state
        .session_manager
        .as_ref()
        .ok_or("Session manager not initialized")?;

    manager
        .get_sessions_in_range(start, end)
        .await
        .map_err(|e| format!("Failed to get session history: {}", e))
}

#[tauri::command]
async fn get_session_metrics(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<SessionMetrics, String> {
    let manager = state
        .session_manager
        .as_ref()
        .ok_or("Session manager not initialized")?;

    manager
        .calculate_session_metrics(&session_id)
        .await
        .map_err(|e| format!("Failed to get session metrics: {}", e))
}

#[tauri::command]
async fn classify_session(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let manager = state
        .session_manager
        .as_ref()
        .ok_or("Session manager not initialized")?;

    let session_type = manager
        .classify_session_type(&session_id)
        .await
        .map_err(|e| format!("Failed to classify session: {}", e))?;

    Ok(session_type.to_string().to_string())
}

#[tauri::command]
async fn end_current_session(state: State<'_, AppState>) -> Result<(), String> {
    let manager = state
        .session_manager
        .as_ref()
        .ok_or("Session manager not initialized")?;

    manager
        .end_current_session()
        .await
        .map_err(|e| format!("Failed to end session: {}", e))
}

#[tauri::command]
async fn start_session_monitoring(state: State<'_, AppState>) -> Result<(), String> {
    let manager = state
        .session_manager
        .as_ref()
        .ok_or("Session manager not initialized")?;

    manager
        .start_monitoring()
        .await
        .map_err(|e| format!("Failed to start session monitoring: {}", e))
}

#[tauri::command]
async fn stop_session_monitoring(state: State<'_, AppState>) -> Result<(), String> {
    let manager = state
        .session_manager
        .as_ref()
        .ok_or("Session manager not initialized")?;

    manager
        .stop_monitoring()
        .await
        .map_err(|e| format!("Failed to stop session monitoring: {}", e))
}

// Keyboard recording commands
#[tauri::command]
async fn start_keyboard_recording(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let recorder = state
        .keyboard_recorder
        .as_ref()
        .ok_or("Keyboard recorder not initialized")?;

    recorder
        .start_recording(session_id)
        .await
        .map_err(|e| format!("Failed to start keyboard recording: {}", e))
}

#[tauri::command]
async fn stop_keyboard_recording(state: State<'_, AppState>) -> Result<(), String> {
    let recorder = state
        .keyboard_recorder
        .as_ref()
        .ok_or("Keyboard recorder not initialized")?;

    recorder
        .stop_recording()
        .await
        .map_err(|e| format!("Failed to stop keyboard recording: {}", e))
}

#[tauri::command]
async fn get_keyboard_stats(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<KeyboardStats, String> {
    let recorder = state
        .keyboard_recorder
        .as_ref()
        .ok_or("Keyboard recorder not initialized")?;

    recorder
        .get_keyboard_stats(session_id)
        .await
        .map_err(|e| format!("Failed to get keyboard stats: {}", e))
}

#[tauri::command]
async fn is_keyboard_recording(state: State<'_, AppState>) -> Result<bool, String> {
    let recorder = state
        .keyboard_recorder
        .as_ref()
        .ok_or("Keyboard recorder not initialized")?;

    Ok(recorder.is_recording().await)
}

// Input recording commands
#[tauri::command]
async fn start_input_recording(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let recorder = state
        .input_recorder
        .as_ref()
        .ok_or("Input recorder not initialized")?;

    recorder
        .start_recording(session_id)
        .await
        .map_err(|e| format!("Failed to start input recording: {}", e))
}

#[tauri::command]
async fn stop_input_recording(state: State<'_, AppState>) -> Result<(), String> {
    let recorder = state
        .input_recorder
        .as_ref()
        .ok_or("Input recorder not initialized")?;

    recorder
        .stop_recording()
        .await
        .map_err(|e| format!("Failed to stop input recording: {}", e))
}

#[tauri::command]
async fn is_input_recording(state: State<'_, AppState>) -> Result<bool, String> {
    let recorder = state
        .input_recorder
        .as_ref()
        .ok_or("Input recorder not initialized")?;

    Ok(recorder.is_recording().await)
}

#[tauri::command]
async fn cleanup_old_input_events(
    retention_days: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let recorder = state
        .input_recorder
        .as_ref()
        .ok_or("Input recorder not initialized")?;

    recorder
        .cleanup_old_events(retention_days)
        .await
        .map_err(|e| format!("Failed to cleanup old events: {}", e))
}

// Command analyzer commands
#[tauri::command]
async fn get_command_stats(
    session_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<CommandStats, String> {
    let db = &state.db;

    let session_uuid = if let Some(sid) = session_id {
        Some(Uuid::parse_str(&sid).map_err(|e| format!("Invalid session ID: {}", e))?)
    } else {
        None
    };

    CommandAnalyzer::get_command_stats(db, session_uuid)
        .await
        .map_err(|e| format!("Failed to get command stats: {}", e))
}

#[tauri::command]
async fn get_most_used_shortcuts(
    limit: u32,
    state: State<'_, AppState>,
) -> Result<Vec<(String, u32)>, String> {
    let stats = get_command_stats(None, state).await?;
    Ok(stats
        .most_used_shortcuts
        .into_iter()
        .take(limit as usize)
        .collect())
}

// Search engine commands
#[tauri::command]
async fn search_text(
    query: String,
    filters: SearchFilters,
    limit: u32,
    offset: u32,
    state: State<'_, AppState>,
) -> Result<SearchResults, String> {
    state
        .search_engine
        .search(SearchQuery {
            query,
            filters,
            limit,
            offset,
        })
        .await
        .map_err(|e| format!("Search failed: {}", e))
}

#[tauri::command]
async fn search_suggestions(
    partial: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    state
        .search_engine
        .suggest_queries(&partial)
        .await
        .map_err(|e| format!("Failed to get suggestions: {}", e))
}

#[tauri::command]
async fn search_in_session(
    session_id: String,
    query: String,
    state: State<'_, AppState>,
) -> Result<SearchResults, String> {
    let session_uuid =
        Uuid::parse_str(&session_id).map_err(|e| format!("Invalid session ID: {}", e))?;

    state
        .search_engine
        .search(SearchQuery {
            query,
            filters: SearchFilters {
                session_ids: Some(vec![session_uuid]),
                ..Default::default()
            },
            limit: 50,
            offset: 0,
        })
        .await
        .map_err(|e| format!("Search failed: {}", e))
}

// Timeline commands
#[tauri::command]
async fn get_timeline_data(
    start_timestamp: i64,
    end_timestamp: i64,
    state: State<'_, AppState>,
) -> Result<TimelineData, String> {
    let manager = state
        .session_manager
        .as_ref()
        .ok_or("Session manager not initialized")?;

    // Get sessions in range
    let sessions = manager
        .get_sessions_in_range(start_timestamp, end_timestamp)
        .await
        .map_err(|e| format!("Failed to get sessions: {}", e))?;

    let mut timeline_sessions = Vec::new();

    for session in &sessions {
        // Get app usage for this session
        let apps = get_app_usage_for_session(&state.db, &session.id).await?;

        // Convert to AppUsageSegments with colors
        let app_segments: Vec<AppUsageSegment> = apps
            .into_iter()
            .map(|app| AppUsageSegment {
                app_name: app.app_name.clone(),
                bundle_id: app.bundle_id.clone(),
                start_timestamp: app.start_timestamp,
                end_timestamp: app
                    .end_timestamp
                    .unwrap_or(chrono::Utc::now().timestamp_millis()),
                focus_duration: app.focus_duration_ms,
                color: app_color(&app.app_name),
            })
            .collect();

        // Calculate activity intensity
        let activity_intensity = calculate_activity_intensity(&app_segments);

        // Check for recordings
        let has_screen_recording = check_has_screen_recording(&state.db, &session.id).await?;
        let has_input_recording = check_has_input_recording(&state.db, &session.id).await?;

        timeline_sessions.push(TimelineSession {
            id: session.id.clone(),
            start_timestamp: session.start_timestamp,
            end_timestamp: session.end_timestamp,
            session_type: session.session_type.clone(),
            applications: app_segments,
            activity_intensity,
            has_screen_recording,
            has_input_recording,
        });
    }

    // Calculate total duration
    let total_duration: u64 = timeline_sessions
        .iter()
        .map(|s| {
            let end = s
                .end_timestamp
                .unwrap_or(chrono::Utc::now().timestamp_millis());
            (end - s.start_timestamp) as u64
        })
        .sum();

    Ok(TimelineData {
        sessions: timeline_sessions,
        total_duration,
        date_range: DateRange {
            start: start_timestamp,
            end: end_timestamp,
        },
    })
}

// Helper functions for timeline
async fn get_app_usage_for_session(
    db: &Arc<Database>,
    session_id: &str,
) -> Result<Vec<core::os_activity::AppUsage>, String> {
    sqlx::query_as::<_, core::os_activity::AppUsage>(
        r#"
        SELECT id, session_id, app_name, bundle_id, process_id,
               start_timestamp, end_timestamp, focus_duration_ms, background_duration_ms
        FROM app_usage
        WHERE session_id = ?
        ORDER BY start_timestamp ASC
        "#,
    )
    .bind(session_id)
    .fetch_all(&db.pool)
    .await
    .map_err(|e| format!("Failed to get app usage: {}", e))
}

async fn check_has_screen_recording(db: &Arc<Database>, session_id: &str) -> Result<bool, String> {
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM screen_recordings WHERE session_id = ?
        "#,
    )
    .bind(session_id)
    .fetch_one(&db.pool)
    .await
    .map_err(|e| format!("Failed to check screen recording: {}", e))?;

    Ok(count > 0)
}

async fn check_has_input_recording(db: &Arc<Database>, session_id: &str) -> Result<bool, String> {
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM keyboard_events WHERE session_id = ? LIMIT 1
        "#,
    )
    .bind(session_id)
    .fetch_one(&db.pool)
    .await
    .map_err(|e| format!("Failed to check input recording: {}", e))?;

    Ok(count > 0)
}

fn app_color(app_name: &str) -> String {
    let mut hasher = DefaultHasher::new();
    app_name.hash(&mut hasher);
    let hash = hasher.finish();

    let hue = (hash % 360) as f32;
    let saturation = 70.0;
    let lightness = 60.0;

    format!("hsl({}, {}%, {}%)", hue, saturation, lightness)
}

fn calculate_activity_intensity(apps: &[AppUsageSegment]) -> f32 {
    // Calculate based on number of app switches
    let app_switches = apps.len() as f32;
    let normalized = (app_switches / 20.0).min(1.0); // Cap at 20 switches
    normalized
}

// Input event DTOs for overlay
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct KeyboardEventDto {
    id: String,
    timestamp: i64,
    event_type: String,
    key_char: Option<String>,
    key_code: i64,
    modifiers: ModifierDto,
    app_name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ModifierDto {
    ctrl: bool,
    shift: bool,
    alt: bool,
    meta: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct MouseEventDto {
    id: String,
    timestamp: i64,
    event_type: String,
    position: PositionDto,
    button: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PositionDto {
    x: i64,
    y: i64,
}

#[derive(sqlx::FromRow)]
struct KeyboardEventRow {
    id: String,
    timestamp: i64,
    event_type: String,
    key_char: Option<String>,
    key_code: i64,
    modifiers_ctrl: bool,
    modifiers_shift: bool,
    modifiers_alt: bool,
    modifiers_meta: bool,
    app_name: String,
}

#[derive(sqlx::FromRow)]
struct MouseEventRow {
    id: String,
    timestamp: i64,
    event_type: String,
    position_x: i64,
    position_y: i64,
    button: Option<String>,
}

// Input event range queries for playback overlay
#[tauri::command]
async fn get_keyboard_events_in_range(
    session_id: String,
    start_time: i64,
    end_time: i64,
    state: State<'_, AppState>,
) -> Result<Vec<KeyboardEventDto>, String> {
    let rows = sqlx::query_as::<_, KeyboardEventRow>(
        r#"
        SELECT id, timestamp, event_type, key_char, key_code,
               modifiers_ctrl, modifiers_shift, modifiers_alt, modifiers_meta,
               app_name
        FROM keyboard_events
        WHERE session_id = ?
          AND timestamp >= ?
          AND timestamp <= ?
        ORDER BY timestamp ASC
        LIMIT 100
        "#,
    )
    .bind(&session_id)
    .bind(start_time)
    .bind(end_time)
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| format!("Failed to get keyboard events: {}", e))?;

    let events = rows
        .into_iter()
        .map(|row| KeyboardEventDto {
            id: row.id,
            timestamp: row.timestamp,
            event_type: row.event_type,
            key_char: row.key_char,
            key_code: row.key_code,
            modifiers: ModifierDto {
                ctrl: row.modifiers_ctrl,
                shift: row.modifiers_shift,
                alt: row.modifiers_alt,
                meta: row.modifiers_meta,
            },
            app_name: row.app_name,
        })
        .collect();

    Ok(events)
}

#[tauri::command]
async fn get_mouse_events_in_range(
    session_id: String,
    start_time: i64,
    end_time: i64,
    state: State<'_, AppState>,
) -> Result<Vec<MouseEventDto>, String> {
    let rows = sqlx::query_as::<_, MouseEventRow>(
        r#"
        SELECT id, timestamp, event_type, position_x, position_y, button
        FROM mouse_events
        WHERE session_id = ?
          AND timestamp >= ?
          AND timestamp <= ?
        ORDER BY timestamp ASC
        LIMIT 100
        "#,
    )
    .bind(&session_id)
    .bind(start_time)
    .bind(end_time)
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| format!("Failed to get mouse events: {}", e))?;

    let events = rows
        .into_iter()
        .map(|row| MouseEventDto {
            id: row.id,
            timestamp: row.timestamp,
            event_type: row.event_type,
            position: PositionDto {
                x: row.position_x,
                y: row.position_y,
            },
            button: row.button,
        })
        .collect();

    Ok(events)
}

// Playback commands
#[tauri::command]
async fn get_playback_info(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<PlaybackInfo, String> {
    let engine = state
        .playback_engine
        .as_ref()
        .ok_or("Playback engine not initialized")?;

    let uuid = Uuid::parse_str(&session_id).map_err(|e| format!("Invalid session ID: {}", e))?;

    engine
        .get_playback_info(uuid)
        .await
        .map_err(|e| format!("Failed to get playback info: {}", e))
}

#[tauri::command]
async fn seek_to_timestamp(
    session_id: String,
    timestamp: i64,
    state: State<'_, AppState>,
) -> Result<SeekInfo, String> {
    let engine = state
        .playback_engine
        .as_ref()
        .ok_or("Playback engine not initialized")?;

    let uuid = Uuid::parse_str(&session_id).map_err(|e| format!("Invalid session ID: {}", e))?;

    engine
        .seek_to_timestamp(uuid, timestamp)
        .await
        .map_err(|e| format!("Failed to seek: {}", e))
}

#[tauri::command]
async fn get_frame_at_timestamp(
    session_id: String,
    timestamp: i64,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let engine = state
        .playback_engine
        .as_ref()
        .ok_or("Playback engine not initialized")?;

    let uuid = Uuid::parse_str(&session_id).map_err(|e| format!("Invalid session ID: {}", e))?;

    engine
        .get_frame_at_timestamp(uuid, timestamp)
        .await
        .map_err(|e| format!("Failed to get frame: {}", e))
}

#[tauri::command]
async fn get_recordings(state: State<'_, AppState>) -> Result<Vec<RecordingInfo>, String> {
    sqlx::query_as::<_, RecordingInfo>(
        r#"SELECT s.id as session_id, s.start_timestamp, s.end_timestamp,
                  COUNT(vs.id) as segment_count,
                  COALESCE(SUM(vs.file_size_bytes), 0) as total_size_bytes,
                  (COALESCE(s.end_timestamp, strftime('%s', 'now')) - s.start_timestamp) * 1000 as total_duration_ms,
                  COALESCE(SUM(vs.frame_count), 0) as frame_count
           FROM sessions s
           INNER JOIN video_segments vs ON vs.session_id = s.id
           GROUP BY s.id
           ORDER BY s.start_timestamp DESC"#,
    )
    .fetch_all(state.db.pool())
    .await
    .map_err(|e| format!("Failed to get recordings: {}", e))
}

#[tauri::command]
async fn delete_recording(session_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let storage = state.storage.as_ref().ok_or("Storage not initialized")?;
    let uuid = Uuid::parse_str(&session_id).map_err(|e| format!("Invalid session ID: {}", e))?;
    storage
        .delete_session(uuid)
        .await
        .map_err(|e| format!("Failed to delete recording: {}", e))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Initialize database, consent manager, config, and screen recorder
            tauri::async_runtime::block_on(async {
                let db = Arc::new(
                    Database::init()
                        .await
                        .expect("Failed to initialize database"),
                );

                let consent_manager = Arc::new(
                    ConsentManager::new(db.clone())
                        .await
                        .expect("Failed to initialize consent manager"),
                );

                let config = Config::load().expect("Failed to load configuration");
                context_timeline::init_schema(&db)
                    .await
                    .expect("Failed to initialize context timeline schema");

                // Initialize recording storage
                let platform = get_platform();
                let data_dir = platform
                    .get_data_directory()
                    .expect("Failed to get data directory");
                let recordings_path = data_dir.join("recordings");

                let storage = Arc::new(
                    RecordingStorage::new(recordings_path, db.clone())
                        .await
                        .expect("Failed to initialize recording storage"),
                );

                // Try to initialize OCR pipeline
                let ocr_processor = match OcrEngine::new(OcrConfig {
                    languages: config.ocr_languages.clone(),
                    confidence_threshold: config.ocr_confidence_threshold,
                    ..OcrConfig::for_screenshots()
                }) {
                    Ok(engine) => {
                        let processor = Arc::new(OcrProcessor::new(
                            Arc::new(engine),
                            Arc::new(OcrStorage::new(db.clone())),
                            OcrProcessorConfig {
                                enabled: true,
                                interval_seconds: config.ocr_interval_seconds,
                                ..OcrProcessorConfig::default()
                            },
                        ));
                        if let Err(error) = processor.start().await {
                            eprintln!("Warning: Failed to start OCR processor: {}", error);
                            None
                        } else {
                            println!("OCR processor initialized successfully");
                            Some(processor)
                        }
                    }
                    Err(error) => {
                        eprintln!("Warning: Failed to initialize OCR engine: {}", error);
                        eprintln!(
                            "OCR capture will be unavailable until Tesseract initializes cleanly"
                        );
                        None
                    }
                };

                let ocr_trigger_signals = Arc::new(OcrTriggerSignals::new());

                // Try to initialize screen recorder (may fail on some platforms)
                let screen_recorder = match ScreenRecorder::new(
                    consent_manager.clone(),
                    storage.clone(),
                    ocr_trigger_signals.clone(),
                )
                .await
                {
                    Ok(recorder) => {
                        if let Some(ocr_processor) = ocr_processor.clone() {
                            recorder.attach_ocr_processor(ocr_processor).await;
                        }
                        println!("Screen recorder initialized successfully");
                        Some(recorder)
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to initialize screen recorder: {}", e);
                        eprintln!("Screen recording features will be unavailable");
                        None
                    }
                };

                // Try to initialize OS activity recorder
                let os_activity_recorder =
                    match OsActivityRecorder::new(consent_manager.clone(), db.clone()).await {
                        Ok(recorder) => {
                            println!("OS activity recorder initialized successfully");
                            Some(Arc::new(recorder))
                        }
                        Err(e) => {
                            eprintln!("Warning: Failed to initialize OS activity recorder: {}", e);
                            eprintln!("OS activity monitoring features will be unavailable");
                            None
                        }
                    };

                if let (Some(screen_recorder), Some(os_activity_recorder)) =
                    (screen_recorder.as_ref(), os_activity_recorder.as_ref())
                {
                    screen_recorder
                        .attach_os_activity_recorder(os_activity_recorder.clone())
                        .await;
                }

                // Initialize session manager
                let session_manager =
                    match SessionManager::new(db.clone(), SessionConfig::default()).await {
                        Ok(manager) => {
                            println!("Session manager initialized successfully");
                            Some(Arc::new(manager))
                        }
                        Err(e) => {
                            eprintln!("Warning: Failed to initialize session manager: {}", e);
                            eprintln!("Session management features will be unavailable");
                            None
                        }
                    };

                // Initialize keyboard recorder
                let keyboard_recorder =
                    match KeyboardRecorder::new(consent_manager.clone(), db.clone()).await {
                        Ok(recorder) => {
                            println!("Keyboard recorder initialized successfully");
                            Some(Arc::new(recorder))
                        }
                        Err(e) => {
                            eprintln!("Warning: Failed to initialize keyboard recorder: {}", e);
                            eprintln!("Keyboard recording features will be unavailable");
                            None
                        }
                    };

                // Initialize input recorder
                let input_recorder = match InputRecorder::new(
                    consent_manager.clone(),
                    db.clone(),
                    ocr_trigger_signals.clone(),
                )
                .await
                {
                    Ok(recorder) => {
                        println!("Input recorder initialized successfully");
                        Some(Arc::new(recorder))
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to initialize input recorder: {}", e);
                        eprintln!("Input recording features will be unavailable");
                        None
                    }
                };

                // Initialize search engine
                let search_engine = Arc::new(SearchEngine::new(db.clone()));
                println!("Search engine initialized successfully");

                // Initialize playback engine
                let playback_engine = Arc::new(PlaybackEngine::new(storage.clone(), db.clone()));
                println!("Playback engine initialized successfully");

                app.manage(AppState {
                    db,
                    consent_manager,
                    config: Arc::new(Mutex::new(config)),
                    screen_recorder,
                    os_activity_recorder,
                    session_manager,
                    keyboard_recorder,
                    input_recorder,
                    search_engine,
                    playback_engine: Some(playback_engine),
                    storage: Some(storage),
                    ocr_processor,
                    desktop_capture_runtime: Arc::new(
                        RwLock::new(DesktopCaptureRuntime::default()),
                    ),
                });
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            check_consent_status,
            request_consent,
            revoke_consent,
            get_all_consents,
            get_config,
            update_config,
            reset_config,
            get_available_displays,
            start_screen_recording,
            stop_screen_recording,
            get_recording_status,
            start_desktop_capture,
            stop_desktop_capture,
            get_desktop_capture_status,
            get_channel_statuses,
            get_capture_data_overview,
            reveal_capture_data_target,
            delete_capture_data,
            get_capture_channel_preview,
            start_os_monitoring,
            stop_os_monitoring,
            get_app_usage_stats,
            get_running_applications,
            get_current_application,
            get_current_session,
            get_session_history,
            get_session_metrics,
            classify_session,
            end_current_session,
            start_session_monitoring,
            stop_session_monitoring,
            start_keyboard_recording,
            stop_keyboard_recording,
            get_keyboard_stats,
            is_keyboard_recording,
            start_input_recording,
            stop_input_recording,
            is_input_recording,
            cleanup_old_input_events,
            get_command_stats,
            get_most_used_shortcuts,
            search_text,
            search_suggestions,
            search_in_session,
            get_timeline_data,
            get_context_timeline,
            get_context_inspector,
            get_context_slice_detail,
            get_scene_snapshot,
            get_scene_snapshots,
            get_text_spans,
            get_context_entities,
            search_agent_context,
            get_activity_episode,
            get_ocr_agent_summary,
            get_app_usage_overview,
            get_pii_review,
            get_ocr_review,
            get_keyboard_events_in_range,
            get_mouse_events_in_range,
            get_playback_info,
            seek_to_timestamp,
            get_frame_at_timestamp,
            get_recordings,
            delete_recording
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
