use crate::app::state::AppState;
use crate::core::config::Config;
use crate::core::consent::Feature;
use crate::core::screen_recorder::RecordingStatus;
use crate::models::capture::Display;
use tauri::State;

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
pub async fn check_consent_status(
    feature: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let feature = Feature::from_string(&feature).map_err(|e| format!("Invalid feature: {}", e))?;
    state
        .consent_manager
        .is_consent_granted(feature)
        .await
        .map_err(|e| format!("Failed to check consent: {}", e))
}

#[tauri::command]
pub async fn request_consent(feature: String, state: State<'_, AppState>) -> Result<(), String> {
    let feature = Feature::from_string(&feature).map_err(|e| format!("Invalid feature: {}", e))?;
    state
        .consent_manager
        .grant_consent(feature)
        .await
        .map_err(|e| format!("Failed to grant consent: {}", e))
}

#[tauri::command]
pub async fn revoke_consent(feature: String, state: State<'_, AppState>) -> Result<(), String> {
    let feature = Feature::from_string(&feature).map_err(|e| format!("Invalid feature: {}", e))?;
    state
        .consent_manager
        .revoke_consent(feature)
        .await
        .map_err(|e| format!("Failed to revoke consent: {}", e))
}

#[tauri::command]
pub async fn get_all_consents(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, bool>, String> {
    let consents = state
        .consent_manager
        .get_all_consents()
        .await
        .map_err(|e| format!("Failed to get consents: {}", e))?;
    Ok(consents
        .into_iter()
        .map(|(feature, granted)| (feature.to_db_string().to_string(), granted))
        .collect())
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<Config, String> {
    state
        .config
        .lock()
        .map(|config| config.clone())
        .map_err(|e| format!("Failed to lock config: {}", e))
}

#[tauri::command]
pub fn update_config(config: Config, state: State<'_, AppState>) -> Result<(), String> {
    config
        .validate()
        .map_err(|e| format!("Invalid configuration: {}", e))?;
    let mut current = state
        .config
        .lock()
        .map_err(|e| format!("Failed to lock config: {}", e))?;
    *current = config.clone();
    config
        .save()
        .map_err(|e| format!("Failed to save configuration: {}", e))
}

#[tauri::command]
pub fn reset_config(state: State<'_, AppState>) -> Result<Config, String> {
    let config = Config::default();
    config
        .save()
        .map_err(|e| format!("Failed to save default config: {}", e))?;
    let mut current = state
        .config
        .lock()
        .map_err(|e| format!("Failed to lock config: {}", e))?;
    *current = config.clone();
    Ok(config)
}

#[tauri::command]
pub async fn get_available_displays(state: State<'_, AppState>) -> Result<Vec<Display>, String> {
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
pub async fn start_screen_recording(
    display_id: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
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
pub async fn stop_screen_recording(state: State<'_, AppState>) -> Result<(), String> {
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
pub async fn get_recording_status(state: State<'_, AppState>) -> Result<RecordingStatus, String> {
    let recorder = state
        .screen_recorder
        .as_ref()
        .ok_or("Screen recorder not initialized")?;
    recorder
        .get_status()
        .await
        .map_err(|e| format!("Failed to get status: {}", e))
}
