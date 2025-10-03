pub mod core;
pub mod platform;

use core::consent::{ConsentManager, Feature};
use core::config::Config;
use core::database::Database;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{Manager, State};

// Application state
pub struct AppState {
    pub consent_manager: ConsentManager,
    pub config: Mutex<Config>,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Initialize database, consent manager, and config
            tauri::async_runtime::block_on(async {
                let db = Database::init()
                    .await
                    .expect("Failed to initialize database");

                let consent_manager = ConsentManager::new(db)
                    .await
                    .expect("Failed to initialize consent manager");

                let config = Config::load()
                    .expect("Failed to load configuration");

                app.manage(AppState {
                    consent_manager,
                    config: Mutex::new(config),
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
            reset_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
