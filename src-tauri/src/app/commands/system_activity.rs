use crate::app::state::AppState;
use crate::core::os_activity::AppUsageStats;
use crate::core::session_manager::{Session, SessionMetrics};
use crate::models::activity::AppInfo;
use crate::models::input::KeyboardStats;
use tauri::State;
use uuid::Uuid;

#[tauri::command]
pub async fn start_os_monitoring(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
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
pub async fn stop_os_monitoring(state: State<'_, AppState>) -> Result<(), String> {
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
pub async fn get_app_usage_stats(
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
pub async fn get_running_applications(state: State<'_, AppState>) -> Result<Vec<AppInfo>, String> {
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
pub async fn get_current_application(
    state: State<'_, AppState>,
) -> Result<Option<AppInfo>, String> {
    let recorder = state
        .os_activity_recorder
        .as_ref()
        .ok_or("OS activity recorder not initialized")?;
    recorder
        .get_current_app()
        .await
        .map_err(|e| format!("Failed to get current app: {}", e))
}

#[tauri::command]
pub async fn get_current_session(state: State<'_, AppState>) -> Result<Option<Session>, String> {
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
pub async fn get_session_history(
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
pub async fn get_session_metrics(
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
pub async fn classify_session(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let manager = state
        .session_manager
        .as_ref()
        .ok_or("Session manager not initialized")?;
    manager
        .classify_session_type(&session_id)
        .await
        .map(|session_type| session_type.to_string().to_string())
        .map_err(|e| format!("Failed to classify session: {}", e))
}

#[tauri::command]
pub async fn end_current_session(state: State<'_, AppState>) -> Result<(), String> {
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
pub async fn start_session_monitoring(state: State<'_, AppState>) -> Result<(), String> {
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
pub async fn stop_session_monitoring(state: State<'_, AppState>) -> Result<(), String> {
    let manager = state
        .session_manager
        .as_ref()
        .ok_or("Session manager not initialized")?;
    manager
        .stop_monitoring()
        .await
        .map_err(|e| format!("Failed to stop session monitoring: {}", e))
}

#[tauri::command]
pub async fn start_keyboard_recording(
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
pub async fn stop_keyboard_recording(state: State<'_, AppState>) -> Result<(), String> {
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
pub async fn get_keyboard_stats(
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
pub async fn is_keyboard_recording(state: State<'_, AppState>) -> Result<bool, String> {
    let recorder = state
        .keyboard_recorder
        .as_ref()
        .ok_or("Keyboard recorder not initialized")?;
    Ok(recorder.is_recording().await)
}

#[tauri::command]
pub async fn start_input_recording(
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
pub async fn stop_input_recording(state: State<'_, AppState>) -> Result<(), String> {
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
pub async fn is_input_recording(state: State<'_, AppState>) -> Result<bool, String> {
    let recorder = state
        .input_recorder
        .as_ref()
        .ok_or("Input recorder not initialized")?;
    Ok(recorder.is_recording().await)
}

#[tauri::command]
pub async fn cleanup_old_input_events(
    days_to_keep: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let recorder = state
        .input_recorder
        .as_ref()
        .ok_or("Input recorder not initialized")?;
    recorder
        .cleanup_old_events(days_to_keep)
        .await
        .map_err(|e| format!("Failed to clean up old input events: {}", e))
}
