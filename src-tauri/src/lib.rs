pub mod core;
pub mod models;
pub mod platform;

use core::consent::{ConsentManager, Feature};
use core::config::Config;
use core::database::Database;
use core::os_activity::{AppUsageStats, OsActivityRecorder};
use core::screen_recorder::{RecordingStatus, ScreenRecorder};
use core::session_manager::{Session, SessionConfig, SessionManager, SessionMetrics};
use core::storage::RecordingStorage;
use models::activity::AppInfo;
use models::capture::Display;
use platform::get_platform;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

// Application state
pub struct AppState {
    pub consent_manager: Arc<ConsentManager>,
    pub config: Mutex<Config>,
    pub screen_recorder: Option<ScreenRecorder>,
    pub os_activity_recorder: Option<Arc<OsActivityRecorder>>,
    pub session_manager: Option<Arc<SessionManager>>,
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

                app.manage(AppState {
                    consent_manager,
                    config: Mutex::new(config),
                    screen_recorder,
                    os_activity_recorder,
                    session_manager,
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
            stop_session_monitoring
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
