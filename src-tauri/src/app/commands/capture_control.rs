use crate::app::capture::sampler::{spawn_desktop_sampler, start_multimodal_capture};
use crate::app::capture::status::{build_channel_statuses, build_desktop_capture_status};
use crate::app::state::{AppState, ChannelStatusDto, DesktopCaptureStatusDto};
use crate::core::context_timeline;
use tauri::State;

#[tauri::command]
pub async fn start_desktop_capture(
    display_id: Option<u32>,
    state: State<'_, AppState>,
) -> Result<DesktopCaptureStatusDto, String> {
    let session_manager = state
        .session_manager
        .as_ref()
        .ok_or("Session manager not initialized")?;
    let config = state
        .config
        .lock()
        .map_err(|e| format!("Failed to lock config: {}", e))?
        .clone();
    let current_generation = state
        .desktop_capture_runtime
        .read()
        .await
        .sampler_generation
        .saturating_add(1);
    let session_id = session_manager
        .get_or_create_session()
        .await
        .map_err(|e| format!("Failed to start session: {}", e))?;
    let started_at = chrono::Utc::now().timestamp_millis();
    let mut started_any_channel = false;

    {
        let mut runtime = state.desktop_capture_runtime.write().await;
        runtime.is_active = true;
        runtime.session_id = Some(session_id.clone());
        runtime.started_at = Some(started_at);
        runtime.display_id = display_id;
        runtime.display_name = None;
        runtime.audio_source_name = None;
        runtime.warnings.clear();
        runtime.channel_errors.clear();
    }

    if config.capture_channels.screen_frames {
        let display_id = display_id.ok_or(
            "Screen capture is enabled, but no display is selected. Disable screen capture or select a display.",
        )?;
        if let Some(recorder) = state.screen_recorder.as_ref() {
            recorder
                .start_recording(display_id)
                .await
                .map_err(|e| format!("Failed to start screen recording: {}", e))?;
            started_any_channel = true;
            if let Ok(displays) = recorder.get_available_displays().await {
                if let Some(display) = displays.iter().find(|item| item.id == display_id) {
                    state.desktop_capture_runtime.write().await.display_name =
                        Some(display.name.clone());
                }
            }
        }
    }
    if config.capture_channels.system
        || config.capture_channels.focus
        || config.capture_channels.visible_windows
    {
        if let Some(recorder) = state.os_activity_recorder.as_ref() {
            recorder
                .start_recording(session_id.clone())
                .await
                .map_err(|e| format!("Failed to start OS monitoring: {}", e))?;
            started_any_channel = true;
        }
    }
    if config.capture_channels.keyboard {
        if let Some(recorder) = state.keyboard_recorder.as_ref() {
            recorder
                .start_recording(session_id.clone())
                .await
                .map_err(|e| format!("Failed to start keyboard recording: {}", e))?;
            started_any_channel = true;
        }
    }
    if config.capture_channels.mouse {
        if let Some(recorder) = state.input_recorder.as_ref() {
            recorder
                .start_recording(session_id.clone())
                .await
                .map_err(|e| format!("Failed to start input recording: {}", e))?;
            started_any_channel = true;
        }
    }

    start_multimodal_capture(&state, &session_id, &config, &mut started_any_channel).await;
    finalize_capture_start(
        &state,
        session_manager,
        started_any_channel,
        session_id,
        started_at,
        current_generation,
        config.capture_channels.system,
        config.capture_channels.system
            || config.capture_channels.focus
            || config.capture_channels.visible_windows,
    )
    .await
}

async fn finalize_capture_start(
    state: &AppState,
    session_manager: &std::sync::Arc<crate::core::session_manager::SessionManager>,
    started_any_channel: bool,
    session_id: String,
    started_at: i64,
    current_generation: u64,
    record_system_lifecycle: bool,
    start_context_sampler: bool,
) -> Result<DesktopCaptureStatusDto, String> {
    {
        let mut runtime = state.desktop_capture_runtime.write().await;
        runtime.is_active = started_any_channel;
        if !started_any_channel {
            runtime.session_id = None;
            runtime.started_at = None;
            runtime.display_id = None;
            runtime.display_name = None;
            runtime.audio_source_name = None;
            if runtime.warnings.is_empty() {
                runtime.warnings.push(
                    "No capture channels could start with the current configuration.".to_string(),
                );
            }
        }
    }
    if !started_any_channel {
        let _ = session_manager.end_current_session().await;
        return build_desktop_capture_status(state).await;
    }

    if record_system_lifecycle {
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
    }

    if start_context_sampler {
        let db = state.db.clone();
        let os_activity = state.os_activity_recorder.clone();
        let config_handle = state.config.clone();
        let runtime_handle = state.desktop_capture_runtime.clone();
        tauri::async_runtime::spawn(async move {
            runtime_handle.write().await.sampler_generation = current_generation;
            spawn_desktop_sampler(db, os_activity, config_handle, runtime_handle).await;
        });
    }
    build_desktop_capture_status(state).await
}

#[tauri::command]
pub async fn stop_desktop_capture(
    state: State<'_, AppState>,
) -> Result<DesktopCaptureStatusDto, String> {
    let session_id = state
        .desktop_capture_runtime
        .read()
        .await
        .session_id
        .clone();
    let record_system_lifecycle = state
        .config
        .lock()
        .map(|config| config.capture_channels.system)
        .unwrap_or(false);
    if let Some(recorder) = state.input_recorder.as_ref() {
        let _ = recorder.stop_recording().await;
    }
    if let Some(recorder) = state.os_activity_recorder.as_ref() {
        let _ = recorder.stop_recording().await;
    }
    if let Some(recorder) = state.screen_recorder.as_ref() {
        let _ = recorder.stop_recording().await;
    }
    if let Some(service) = state.multimodal_service.as_ref() {
        service.stop_capture().await;
    }
    if let Some(manager) = state.session_manager.as_ref() {
        let _ = manager.end_current_session().await;
    }
    if record_system_lifecycle {
        if let Some(session_id) = session_id.as_ref() {
            let _ = context_timeline::insert_context_event(&state.db, Some(session_id), chrono::Utc::now().timestamp_millis(), "system", "capture_stopped", "desktop_capture", 1.0, Some(serde_json::json!({"title": "Desktop capture stopped","subtitle": "SOURCE ended the current multi-channel capture session"}).to_string())).await;
        }
    }
    {
        let mut runtime = state.desktop_capture_runtime.write().await;
        runtime.is_active = false;
        runtime.session_id = None;
        runtime.started_at = None;
        runtime.audio_source_name = None;
        runtime.sampler_generation += 1;
    }
    build_desktop_capture_status(&state).await
}

#[tauri::command]
pub async fn get_desktop_capture_status(
    state: State<'_, AppState>,
) -> Result<DesktopCaptureStatusDto, String> {
    build_desktop_capture_status(&state).await
}

#[tauri::command]
pub async fn get_channel_statuses(
    state: State<'_, AppState>,
) -> Result<Vec<ChannelStatusDto>, String> {
    build_channel_statuses(&state).await
}
