pub mod core;
pub mod models;
pub mod platform;

use core::consent::{ConsentManager, Feature};
use core::config::Config;
use core::database::Database;
use core::screen_recorder::{RecordingStatus, ScreenRecorder};
use models::capture::Display;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

// Application state
pub struct AppState {
    pub consent_manager: Arc<ConsentManager>,
    pub config: Mutex<Config>,
    pub screen_recorder: Option<ScreenRecorder>,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Initialize database, consent manager, config, and screen recorder
            tauri::async_runtime::block_on(async {
                let db = Database::init()
                    .await
                    .expect("Failed to initialize database");

                let consent_manager = Arc::new(
                    ConsentManager::new(db)
                        .await
                        .expect("Failed to initialize consent manager")
                );

                let config = Config::load()
                    .expect("Failed to load configuration");

                // Try to initialize screen recorder (may fail on some platforms)
                let screen_recorder = match ScreenRecorder::new(consent_manager.clone()).await {
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

                app.manage(AppState {
                    consent_manager,
                    config: Mutex::new(config),
                    screen_recorder,
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
            get_recording_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
