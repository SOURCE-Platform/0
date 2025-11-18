pub mod core;
pub mod models;
pub mod platform;

use core::command_analyzer::{Command, CommandAnalyzer, CommandStats};
use core::consent::{ConsentManager, Feature};
use core::config::Config;
use core::database::Database;
use core::input_recorder::InputRecorder;
use core::input_storage::{InputTimeline, TimeRange};
use core::keyboard_recorder::KeyboardRecorder;
use core::os_activity::{AppUsageStats, OsActivityRecorder};
use core::playback_engine::{PlaybackEngine, PlaybackInfo, SeekInfo};
use core::screen_recorder::{RecordingStatus, ScreenRecorder};
use core::search_engine::{SearchEngine, SearchFilters, SearchQuery, SearchResults};
use core::session_manager::{Session, SessionConfig, SessionManager, SessionMetrics};
use core::storage::RecordingStorage;

// Pose estimation and audio processing modules
use core::audio_recorder::AudioRecorder;
use core::emotion_detector::EmotionDetector;
use core::pose_detector::PoseDetector;
use core::speaker_diarizer::SpeakerDiarizer;
use core::speech_transcriber::SpeechTranscriber;

use models::activity::AppInfo;
use models::audio::{
    AudioConfig, AudioDevice, EmotionStatistics, SpeakerInfo, TranscriptSearchResult,
    TranscriptSegmentDto, WhisperModelSize, AudioDeviceDto, EmotionResultDto, SpeakerSegmentDto,
    AudioSourceDto,
};
use models::capture::Display;
use models::input::{KeyboardEvent, KeyboardStats, MouseEvent};
use models::pose::{FacialExpressionDto, PoseConfig, PoseFrameDto, PoseStatistics};
use chrono;
use platform::get_platform;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};
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

// Application state
pub struct AppState {
    pub db: Arc<Database>,
    pub consent_manager: Arc<ConsentManager>,
    pub config: Mutex<Config>,
    pub screen_recorder: Option<ScreenRecorder>,
    pub os_activity_recorder: Option<Arc<OsActivityRecorder>>,
    pub session_manager: Option<Arc<SessionManager>>,
    pub keyboard_recorder: Option<Arc<KeyboardRecorder>>,
    pub input_recorder: Option<Arc<InputRecorder>>,
    pub search_engine: Arc<SearchEngine>,
    pub playback_engine: Option<Arc<PlaybackEngine>>,

    // Pose estimation and audio processing modules
    pub pose_detector: Option<Arc<PoseDetector>>,
    pub audio_recorder: Option<Arc<AudioRecorder>>,
    pub speech_transcriber: Option<Arc<SpeechTranscriber>>,
    pub speaker_diarizer: Option<Arc<SpeakerDiarizer>>,
    pub emotion_detector: Option<Arc<EmotionDetector>>,
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

// Consent management commands
#[tauri::command]
async fn check_consent_status(
    feature: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let feature = Feature::from_string(&feature)
        .map_err(|e| format!("Invalid feature: {}", e))?;

    state
        .consent_manager
        .is_consent_granted(feature)
        .await
        .map_err(|e| format!("Failed to check consent: {}", e))
}

#[tauri::command]
async fn request_consent(feature: String, state: State<'_, AppState>) -> Result<(), String> {
    let feature = Feature::from_string(&feature)
        .map_err(|e| format!("Invalid feature: {}", e))?;

    state
        .consent_manager
        .grant_consent(feature)
        .await
        .map_err(|e| format!("Failed to grant consent: {}", e))
}

#[tauri::command]
async fn revoke_consent(feature: String, state: State<'_, AppState>) -> Result<(), String> {
    let feature = Feature::from_string(&feature)
        .map_err(|e| format!("Invalid feature: {}", e))?;

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
    let default_config = Config::reset()
        .map_err(|e| format!("Failed to reset config: {}", e))?;

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
    let recorder = state.screen_recorder.as_ref()
        .ok_or("Screen recorder not initialized")?;

    recorder
        .get_available_displays()
        .await
        .map_err(|e| format!("Failed to get displays: {}", e))
}

#[tauri::command]
async fn start_screen_recording(
    display_id: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let recorder = state.screen_recorder.as_ref()
        .ok_or("Screen recorder not initialized")?;

    recorder
        .start_recording(display_id)
        .await
        .map_err(|e| format!("Failed to start recording: {}", e))
}

#[tauri::command]
async fn stop_screen_recording(state: State<'_, AppState>) -> Result<(), String> {
    let recorder = state.screen_recorder.as_ref()
        .ok_or("Screen recorder not initialized")?;

    recorder
        .stop_recording()
        .await
        .map_err(|e| format!("Failed to stop recording: {}", e))
}

#[tauri::command]
async fn get_recording_status(state: State<'_, AppState>) -> Result<RecordingStatus, String> {
    let recorder = state.screen_recorder.as_ref()
        .ok_or("Screen recorder not initialized")?;

    recorder
        .get_status()
        .await
        .map_err(|e| format!("Failed to get status: {}", e))
}

// OS monitoring commands
#[tauri::command]
async fn start_os_monitoring(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let recorder = state.os_activity_recorder.as_ref()
        .ok_or("OS activity recorder not initialized")?;

    recorder
        .start_recording(session_id)
        .await
        .map_err(|e| format!("Failed to start OS monitoring: {}", e))
}

#[tauri::command]
async fn stop_os_monitoring(state: State<'_, AppState>) -> Result<(), String> {
    let recorder = state.os_activity_recorder.as_ref()
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
    let recorder = state.os_activity_recorder.as_ref()
        .ok_or("OS activity recorder not initialized")?;

    recorder
        .get_app_usage_stats(session_id)
        .await
        .map_err(|e| format!("Failed to get app usage stats: {}", e))
}

#[tauri::command]
async fn get_running_applications(state: State<'_, AppState>) -> Result<Vec<AppInfo>, String> {
    let recorder = state.os_activity_recorder.as_ref()
        .ok_or("OS activity recorder not initialized")?;

    recorder
        .get_running_apps()
        .await
        .map_err(|e| format!("Failed to get running apps: {}", e))
}

#[tauri::command]
async fn get_current_application(state: State<'_, AppState>) -> Result<Option<AppInfo>, String> {
    let recorder = state.os_activity_recorder.as_ref()
        .ok_or("OS activity recorder not initialized")?;

    recorder
        .get_current_app()
        .await
        .map_err(|e| format!("Failed to get current app: {}", e))
}

// Session management commands
#[tauri::command]
async fn get_current_session(state: State<'_, AppState>) -> Result<Option<Session>, String> {
    let manager = state.session_manager.as_ref()
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
    let manager = state.session_manager.as_ref()
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
    let manager = state.session_manager.as_ref()
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
    let manager = state.session_manager.as_ref()
        .ok_or("Session manager not initialized")?;

    let session_type = manager
        .classify_session_type(&session_id)
        .await
        .map_err(|e| format!("Failed to classify session: {}", e))?;

    Ok(session_type.to_string().to_string())
}

#[tauri::command]
async fn end_current_session(state: State<'_, AppState>) -> Result<(), String> {
    let manager = state.session_manager.as_ref()
        .ok_or("Session manager not initialized")?;

    manager
        .end_current_session()
        .await
        .map_err(|e| format!("Failed to end session: {}", e))
}

#[tauri::command]
async fn start_session_monitoring(state: State<'_, AppState>) -> Result<(), String> {
    let manager = state.session_manager.as_ref()
        .ok_or("Session manager not initialized")?;

    manager
        .start_monitoring()
        .await
        .map_err(|e| format!("Failed to start session monitoring: {}", e))
}

#[tauri::command]
async fn stop_session_monitoring(state: State<'_, AppState>) -> Result<(), String> {
    let manager = state.session_manager.as_ref()
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
    let recorder = state.keyboard_recorder.as_ref()
        .ok_or("Keyboard recorder not initialized")?;

    recorder
        .start_recording(session_id)
        .await
        .map_err(|e| format!("Failed to start keyboard recording: {}", e))
}

#[tauri::command]
async fn stop_keyboard_recording(state: State<'_, AppState>) -> Result<(), String> {
    let recorder = state.keyboard_recorder.as_ref()
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
    let recorder = state.keyboard_recorder.as_ref()
        .ok_or("Keyboard recorder not initialized")?;

    recorder
        .get_keyboard_stats(session_id)
        .await
        .map_err(|e| format!("Failed to get keyboard stats: {}", e))
}

#[tauri::command]
async fn is_keyboard_recording(state: State<'_, AppState>) -> Result<bool, String> {
    let recorder = state.keyboard_recorder.as_ref()
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
    let session_uuid = Uuid::parse_str(&session_id)
        .map_err(|e| format!("Invalid session ID: {}", e))?;

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
                end_timestamp: app.end_timestamp.unwrap_or(chrono::Utc::now().timestamp_millis()),
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
            let end = s.end_timestamp.unwrap_or(chrono::Utc::now().timestamp_millis());
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
        "#
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
        "#
    )
    .bind(&session_id)
    .bind(start_time)
    .bind(end_time)
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| format!("Failed to get keyboard events: {}", e))?;

    let events = rows.into_iter().map(|row| KeyboardEventDto {
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
    }).collect();

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
        "#
    )
    .bind(&session_id)
    .bind(start_time)
    .bind(end_time)
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| format!("Failed to get mouse events: {}", e))?;

    let events = rows.into_iter().map(|row| MouseEventDto {
        id: row.id,
        timestamp: row.timestamp,
        event_type: row.event_type,
        position: PositionDto {
            x: row.position_x,
            y: row.position_y,
        },
        button: row.button,
    }).collect();

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

    let uuid = Uuid::parse_str(&session_id)
        .map_err(|e| format!("Invalid session ID: {}", e))?;

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

    let uuid = Uuid::parse_str(&session_id)
        .map_err(|e| format!("Invalid session ID: {}", e))?;

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

    let uuid = Uuid::parse_str(&session_id)
        .map_err(|e| format!("Invalid session ID: {}", e))?;

    engine
        .get_frame_at_timestamp(uuid, timestamp)
        .await
        .map_err(|e| format!("Failed to get frame: {}", e))
}

// ==============================================================================
// Pose Tracking Commands
// ==============================================================================

#[tauri::command]
async fn start_pose_tracking(
    session_id: String,
    config: PoseConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let detector = state
        .pose_detector
        .as_ref()
        .ok_or("Pose detector not initialized")?;

    detector
        .start_tracking(session_id, config)
        .await
        .map_err(|e| format!("Failed to start pose tracking: {}", e))
}

#[tauri::command]
async fn stop_pose_tracking(state: State<'_, AppState>) -> Result<(), String> {
    let detector = state
        .pose_detector
        .as_ref()
        .ok_or("Pose detector not initialized")?;

    detector
        .stop_tracking()
        .await
        .map_err(|e| format!("Failed to stop pose tracking: {}", e))
}

#[tauri::command]
async fn get_pose_frames(
    session_id: String,
    start: i64,
    end: i64,
    state: State<'_, AppState>,
) -> Result<Vec<PoseFrameDto>, String> {
    let detector = state
        .pose_detector
        .as_ref()
        .ok_or("Pose detector not initialized")?;

    detector
        .get_pose_frames(&session_id, start, end)
        .await
        .map_err(|e| format!("Failed to get pose frames: {}", e))
}

#[tauri::command]
async fn get_facial_expressions(
    session_id: String,
    expression_type: Option<String>,
    start: i64,
    end: i64,
    state: State<'_, AppState>,
) -> Result<Vec<FacialExpressionDto>, String> {
    let detector = state
        .pose_detector
        .as_ref()
        .ok_or("Pose detector not initialized")?;

    detector
        .get_facial_expressions(&session_id, expression_type, start, end)
        .await
        .map_err(|e| format!("Failed to get facial expressions: {}", e))
}

#[tauri::command]
async fn get_pose_statistics(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<PoseStatistics, String> {
    let detector = state
        .pose_detector
        .as_ref()
        .ok_or("Pose detector not initialized")?;

    detector
        .get_pose_statistics(&session_id)
        .await
        .map_err(|e| format!("Failed to get pose statistics: {}", e))
}

// ==============================================================================
// Audio Recording Commands
// ==============================================================================

#[tauri::command]
async fn start_audio_recording(
    session_id: String,
    config: AudioConfig,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let recorder = state
        .audio_recorder
        .as_ref()
        .ok_or("Audio recorder not initialized")?;

    recorder
        .start_recording(session_id, config)
        .await
        .map_err(|e| format!("Failed to start audio recording: {}", e))
}

#[tauri::command]
async fn stop_audio_recording(state: State<'_, AppState>) -> Result<(), String> {
    let recorder = state
        .audio_recorder
        .as_ref()
        .ok_or("Audio recorder not initialized")?;

    recorder
        .stop_recording()
        .await
        .map_err(|e| format!("Failed to stop audio recording: {}", e))
}

#[tauri::command]
async fn get_audio_devices() -> Result<Vec<AudioDeviceDto>, String> {
    let devices = AudioRecorder::get_devices()
        .await
        .map_err(|e| format!("Failed to get audio devices: {}", e))?;

    Ok(devices.into_iter().map(|d| AudioDeviceDto {
        id: d.id,
        name: d.name,
        device_type: d.device_type.to_string().to_string(),
        is_default: d.is_default,
    }).collect())
}

// ==============================================================================
// Speech Transcription Commands
// ==============================================================================

#[tauri::command]
async fn get_transcripts(
    session_id: String,
    start: i64,
    end: i64,
    speaker_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<TranscriptSegmentDto>, String> {
    let transcriber = state
        .speech_transcriber
        .as_ref()
        .ok_or("Speech transcriber not initialized")?;

    let segments = transcriber
        .get_transcripts(&session_id, start, end, speaker_id)
        .await
        .map_err(|e| format!("Failed to get transcripts: {}", e))?;

    Ok(segments.into_iter().map(|s| TranscriptSegmentDto {
        timestamp: s.start_timestamp,
        end_timestamp: s.end_timestamp,
        text: s.text,
        language: s.language,
        speaker_id: s.speaker_id,
        confidence: s.confidence,
    }).collect())
}

#[tauri::command]
async fn search_transcripts(
    query: String,
    session_id: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Vec<TranscriptSegmentDto>, String> {
    let transcriber = state
        .speech_transcriber
        .as_ref()
        .ok_or("Speech transcriber not initialized")?;

    let segments = transcriber
        .search_transcripts(&query, session_id, limit.unwrap_or(50), offset.unwrap_or(0))
        .await
        .map_err(|e| format!("Failed to search transcripts: {}", e))?;

    Ok(segments.into_iter().map(|s| TranscriptSegmentDto {
        timestamp: s.start_timestamp,
        end_timestamp: s.end_timestamp,
        text: s.text,
        language: s.language,
        speaker_id: s.speaker_id,
        confidence: s.confidence,
    }).collect())
}

// ==============================================================================
// Speaker Diarization Commands
// ==============================================================================

#[tauri::command]
async fn get_speakers(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<SpeakerInfo>, String> {
    let diarizer = state
        .speaker_diarizer
        .as_ref()
        .ok_or("Speaker diarizer not initialized")?;

    diarizer
        .get_speakers(&session_id)
        .await
        .map_err(|e| format!("Failed to get speakers: {}", e))
}

#[tauri::command]
async fn get_speaker_segments(
    recording_id: String,
    speaker_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<SpeakerSegmentDto>, String> {
    let diarizer = state
        .speaker_diarizer
        .as_ref()
        .ok_or("Speaker diarizer not initialized")?;

    let segments = diarizer
        .get_speaker_segments(&recording_id, speaker_id)
        .await
        .map_err(|e| format!("Failed to get speaker segments: {}", e))?;

    Ok(segments.into_iter().map(|s| SpeakerSegmentDto {
        speaker_id: s.speaker_id,
        start_timestamp: s.start_timestamp,
        end_timestamp: s.end_timestamp,
        confidence: s.confidence,
    }).collect())
}

// ==============================================================================
// Emotion Detection Commands
// ==============================================================================

#[tauri::command]
async fn get_emotions(
    session_id: String,
    start: i64,
    end: i64,
    speaker_id: Option<String>,
    emotion_type: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<EmotionResultDto>, String> {
    let detector = state
        .emotion_detector
        .as_ref()
        .ok_or("Emotion detector not initialized")?;

    let emotions = detector
        .get_emotions(&session_id, start, end, speaker_id, emotion_type)
        .await
        .map_err(|e| format!("Failed to get emotions: {}", e))?;

    Ok(emotions.into_iter().map(|e| EmotionResultDto {
        timestamp: e.timestamp,
        speaker_id: e.speaker_id,
        emotion: e.emotion.to_string().to_string(),
        confidence: e.confidence,
        valence: e.valence,
        arousal: e.arousal,
    }).collect())
}

#[tauri::command]
async fn get_emotion_statistics(
    session_id: String,
    speaker_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<EmotionStatistics, String> {
    let detector = state
        .emotion_detector
        .as_ref()
        .ok_or("Emotion detector not initialized")?;

    detector
        .get_emotion_statistics(&session_id, speaker_id)
        .await
        .map_err(|e| format!("Failed to get emotion statistics: {}", e))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Initialize database, consent manager, config, and screen recorder
            tauri::async_runtime::block_on(async {
                let db = Arc::new(
                    Database::init()
                        .await
                        .expect("Failed to initialize database")
                );

                let consent_manager = Arc::new(
                    ConsentManager::new(db.clone())
                        .await
                        .expect("Failed to initialize consent manager")
                );

                let config = Config::load()
                    .expect("Failed to load configuration");

                // Initialize recording storage
                let platform = get_platform();
                let data_dir = platform.get_data_directory()
                    .expect("Failed to get data directory");
                let recordings_path = data_dir.join("recordings");

                let storage = Arc::new(
                    RecordingStorage::new(recordings_path, db.clone())
                        .await
                        .expect("Failed to initialize recording storage")
                );

                // Try to initialize screen recorder (may fail on some platforms)
                let screen_recorder = match ScreenRecorder::new(consent_manager.clone(), storage.clone()).await {
                    Ok(recorder) => {
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
                let os_activity_recorder = match OsActivityRecorder::new(consent_manager.clone(), db.clone()).await {
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

                // Initialize session manager
                let session_manager = match SessionManager::new(db.clone(), SessionConfig::default()).await {
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
                let keyboard_recorder = match KeyboardRecorder::new(consent_manager.clone(), db.clone()).await {
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
                let input_recorder = match InputRecorder::new(consent_manager.clone(), db.clone()).await {
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

                // Initialize pose detector
                let pose_detector = match PoseDetector::new(consent_manager.clone(), db.clone()).await {
                    Ok(detector) => {
                        println!("Pose detector initialized successfully");
                        Some(Arc::new(detector))
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to initialize pose detector: {}", e);
                        eprintln!("Pose tracking features will be unavailable");
                        None
                    }
                };

                // Initialize audio recorder
                let audio_path = data_dir.join("audio");
                let audio_recorder = match AudioRecorder::new(consent_manager.clone(), db.clone(), audio_path).await {
                    Ok(recorder) => {
                        println!("Audio recorder initialized successfully");
                        Some(Arc::new(recorder))
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to initialize audio recorder: {}", e);
                        eprintln!("Audio recording features will be unavailable");
                        None
                    }
                };

                // Initialize speech transcriber
                let speech_transcriber = match SpeechTranscriber::new(db.clone(), WhisperModelSize::Base).await {
                    Ok(transcriber) => {
                        println!("Speech transcriber initialized successfully");
                        Some(Arc::new(transcriber))
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to initialize speech transcriber: {}", e);
                        eprintln!("Speech transcription features will be unavailable");
                        None
                    }
                };

                // Initialize speaker diarizer
                let speaker_diarizer = match SpeakerDiarizer::new(db.clone()).await {
                    Ok(diarizer) => {
                        println!("Speaker diarizer initialized successfully");
                        Some(Arc::new(diarizer))
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to initialize speaker diarizer: {}", e);
                        eprintln!("Speaker diarization features will be unavailable");
                        None
                    }
                };

                // Initialize emotion detector
                let emotion_detector = match EmotionDetector::new(db.clone()).await {
                    Ok(detector) => {
                        println!("Emotion detector initialized successfully");
                        Some(Arc::new(detector))
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to initialize emotion detector: {}", e);
                        eprintln!("Emotion detection features will be unavailable");
                        None
                    }
                };

                app.manage(AppState {
                    db,
                    consent_manager,
                    config: Mutex::new(config),
                    screen_recorder,
                    os_activity_recorder,
                    session_manager,
                    keyboard_recorder,
                    input_recorder,
                    search_engine,
                    playback_engine: Some(playback_engine),
                    pose_detector,
                    audio_recorder,
                    speech_transcriber,
                    speaker_diarizer,
                    emotion_detector,
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
            get_keyboard_events_in_range,
            get_mouse_events_in_range,
            get_playback_info,
            seek_to_timestamp,
            get_frame_at_timestamp,
            // Pose tracking commands
            start_pose_tracking,
            stop_pose_tracking,
            get_pose_frames,
            get_facial_expressions,
            get_pose_statistics,
            // Audio recording commands
            start_audio_recording,
            stop_audio_recording,
            get_audio_devices,
            // Speech transcription commands
            get_transcripts,
            search_transcripts,
            // Speaker diarization commands
            get_speakers,
            get_speaker_segments,
            // Emotion detection commands
            get_emotions,
            get_emotion_statistics
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
