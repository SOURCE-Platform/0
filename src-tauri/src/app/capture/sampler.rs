use crate::app::capture::status::interval_for_profile;
use crate::app::state::{AppState, DesktopCaptureRuntime};
use crate::core::config::Config;
use crate::core::context_timeline;
use crate::core::database::Database;
use crate::core::multimodal::{MultimodalCaptureOptions, MultimodalStartReport};
use crate::core::os_activity::OsActivityRecorder;
use crate::models::activity::AppInfo;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

pub async fn persist_context_snapshot(
    db: Arc<Database>,
    session_id: Option<String>,
    frontmost: Option<AppInfo>,
    running_apps: Vec<AppInfo>,
    prev_frontmost_bundle_id: &mut Option<String>,
    prev_running_ids: &mut HashSet<String>,
    capture_system: bool,
    capture_focus: bool,
    capture_visible_windows: bool,
) -> Result<(), String> {
    let timestamp = chrono::Utc::now().timestamp_millis();
    if capture_visible_windows {
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
    }

    if capture_focus {
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
    }

    if capture_system {
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
    }
    Ok(())
}

pub async fn spawn_desktop_sampler(
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

        let (interval, capture_system, capture_focus, capture_visible_windows) = config
            .lock()
            .ok()
            .map(|cfg| {
                (
                    interval_for_profile(&cfg.resource_profile),
                    cfg.capture_channels.system,
                    cfg.capture_channels.focus,
                    cfg.capture_channels.visible_windows,
                )
            })
            .unwrap_or((5, false, false, false));

        if capture_system || capture_focus || capture_visible_windows {
            if let Some(recorder) = os_activity_recorder.as_ref() {
                let frontmost = recorder.get_current_app().await.ok().flatten();
                let running_apps = recorder.get_running_apps().await.unwrap_or_default();
                let _ = persist_context_snapshot(
                    db.clone(),
                    session_id.clone(),
                    frontmost,
                    running_apps,
                    &mut prev_frontmost_bundle_id,
                    &mut prev_running_ids,
                    capture_system,
                    capture_focus,
                    capture_visible_windows,
                )
                .await;
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
        if runtime.read().await.sampler_generation != generation {
            break;
        }
    }
}

pub async fn start_multimodal_capture(
    state: &AppState,
    session_id: &str,
    config: &Config,
    started_any_channel: &mut bool,
) {
    if !(config.capture_channels.camera_future || config.capture_channels.audio_future) {
        return;
    }

    let selected_display_id = state.desktop_capture_runtime.read().await.display_id;

    if let Some(service) = state.multimodal_service.as_ref() {
        match service
            .start_capture(
                session_id.to_string(),
                MultimodalCaptureOptions {
                    enable_visual: config.capture_channels.camera_future,
                    enable_audio: config.capture_channels.audio_future,
                    display_id: selected_display_id,
                    audio_source_id: config.selected_audio_input_id.clone(),
                    enable_microphone_audio: config.audio_microphone_enabled,
                    enable_desktop_audio: config.audio_desktop_enabled,
                    desktop_audio_gain_db: config.desktop_audio_gain_db,
                    audio_transcription_enabled: config.audio_transcription_enabled,
                    audio_speech_emotion_enabled: config.audio_speech_emotion_enabled,
                    audio_sound_events_enabled: config.audio_sound_events_enabled,
                },
            )
            .await
        {
            Ok(MultimodalStartReport {
                visual_started,
                audio_started,
                visual_source_name,
                audio_source_name,
                warnings,
            }) => {
                if visual_started || audio_started {
                    *started_any_channel = true;
                }
                let mut runtime = state.desktop_capture_runtime.write().await;
                update_multimodal_runtime(
                    &mut runtime,
                    config,
                    visual_source_name,
                    audio_source_name,
                    warnings,
                );
            }
            Err(error) => {
                let mut runtime = state.desktop_capture_runtime.write().await;
                if config.capture_channels.camera_future {
                    runtime
                        .channel_errors
                        .insert("camera_future".to_string(), error.clone());
                }
                if config.capture_channels.audio_future {
                    runtime
                        .channel_errors
                        .insert("audio_future".to_string(), error.clone());
                }
                runtime
                    .warnings
                    .push(format!("Multimodal capture could not start: {}.", error));
            }
        }
    } else {
        let mut runtime = state.desktop_capture_runtime.write().await;
        if config.capture_channels.camera_future {
            runtime.channel_errors.insert(
                "camera_future".to_string(),
                "Vision capture service is unavailable.".to_string(),
            );
        }
        if config.capture_channels.audio_future {
            runtime.channel_errors.insert(
                "audio_future".to_string(),
                "Audio capture service is unavailable.".to_string(),
            );
        }
        runtime.warnings.push(
            "Camera/audio capture is enabled, but the multimodal service is unavailable."
                .to_string(),
        );
    }
}

fn update_multimodal_runtime(
    runtime: &mut DesktopCaptureRuntime,
    config: &Config,
    visual_source_name: Option<String>,
    audio_source_name: Option<String>,
    warnings: Vec<String>,
) {
    if let Some(source_name) = visual_source_name {
        runtime.channel_errors.remove("camera_future");
        runtime
            .warnings
            .push(format!("Vision capture is sampling from {}.", source_name));
    } else if config.capture_channels.camera_future {
        runtime.channel_errors.insert(
            "camera_future".to_string(),
            "Vision capture could not start with the current device state.".to_string(),
        );
    }
    if let Some(source_name) = audio_source_name {
        runtime.channel_errors.remove("audio_future");
        runtime
            .warnings
            .push(format!("Audio capture is sampling from {}.", source_name));
    } else if config.capture_channels.audio_future {
        runtime.channel_errors.insert(
            "audio_future".to_string(),
            "Audio capture could not start with the current device state.".to_string(),
        );
    }
    runtime.warnings.extend(warnings);
}
