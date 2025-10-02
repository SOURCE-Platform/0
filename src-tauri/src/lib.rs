pub mod core;
pub mod platform;

use core::consent::{ConsentManager, Feature};
use core::database::Database;
use std::collections::HashMap;
use tauri::{Manager, State};

// Application state
pub struct AppState {
    pub consent_manager: ConsentManager,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Initialize database and consent manager
            tauri::async_runtime::block_on(async {
                let db = Database::init()
                    .await
                    .expect("Failed to initialize database");

                let consent_manager = ConsentManager::new(db)
                    .await
                    .expect("Failed to initialize consent manager");

                app.manage(AppState { consent_manager });
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            check_consent_status,
            request_consent,
            revoke_consent,
            get_all_consents
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
