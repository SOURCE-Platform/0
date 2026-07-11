use crate::app::state::{AppState, ChannelStatusDto, DesktopCaptureStatusDto};
use crate::core::config::{Config, ResourceProfile};
use crate::core::consent::{ConsentManager, Feature};
use crate::core::context_timeline;
use std::sync::Arc;

pub fn enabled_channels_from_config(config: &Config) -> Vec<String> {
    let channels = &config.capture_channels;
    [
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
    ]
    .into_iter()
    .filter(|(_, enabled)| *enabled)
    .map(|(name, _)| name.to_string())
    .collect()
}

pub fn interval_for_profile(profile: &ResourceProfile) -> u64 {
    match profile {
        ResourceProfile::Minimal => 15,
        ResourceProfile::Balanced => 5,
        ResourceProfile::HighFidelity => 2,
    }
}

pub async fn channel_permission_state(
    consent_manager: &Arc<ConsentManager>,
    channel: &str,
) -> String {
    let feature = match channel {
        "system" | "focus" | "visible_windows" => Some(Feature::OsActivity),
        "keyboard" => Some(Feature::KeyboardRecording),
        "mouse" => Some(Feature::MouseRecording),
        "screen_frames" | "ocr" => Some(Feature::ScreenRecording),
        "camera_future" => Some(Feature::CameraRecording),
        "audio_future" => Some(Feature::MicrophoneRecording),
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

pub async fn build_desktop_capture_status(
    state: &AppState,
) -> Result<DesktopCaptureStatusDto, String> {
    let runtime = state.desktop_capture_runtime.read().await.clone();
    let config = state
        .config
        .lock()
        .map_err(|e| format!("Failed to lock config: {}", e))?
        .clone();
    let enabled_channels = enabled_channels_from_config(&config);
    let mut missing_permissions = Vec::new();

    for channel in &enabled_channels {
        if channel_permission_state_for_config(&state.consent_manager, channel, &config).await
            == "missing"
        {
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

pub async fn build_channel_statuses(state: &AppState) -> Result<Vec<ChannelStatusDto>, String> {
    let config = state
        .config
        .lock()
        .map_err(|e| format!("Failed to lock config: {}", e))?
        .clone();
    let runtime = state.desktop_capture_runtime.read().await.clone();
    let channels = vec![
        ("system", config.capture_channels.system, "SELECT MAX(timestamp) FROM context_events WHERE channel = 'system'", "SELECT COUNT(*) FROM context_events WHERE channel = 'system' AND timestamp >= strftime('%s','now') * 1000 - 3600000"),
        ("focus", config.capture_channels.focus, "SELECT MAX(timestamp) FROM context_events WHERE channel = 'focus'", "SELECT COUNT(*) FROM context_events WHERE channel = 'focus' AND timestamp >= strftime('%s','now') * 1000 - 3600000"),
        ("visible_windows", config.capture_channels.visible_windows, "SELECT MAX(timestamp) FROM window_snapshots", "SELECT COUNT(*) FROM window_snapshots WHERE timestamp >= strftime('%s','now') * 1000 - 3600000"),
        ("keyboard", config.capture_channels.keyboard, "SELECT MAX(timestamp) FROM keyboard_events", "SELECT COUNT(*) FROM keyboard_events WHERE timestamp >= strftime('%s','now') * 1000 - 3600000"),
        ("mouse", config.capture_channels.mouse, "SELECT MAX(timestamp) FROM mouse_events", "SELECT COUNT(*) FROM mouse_events WHERE timestamp >= strftime('%s','now') * 1000 - 3600000"),
        ("ocr", config.capture_channels.ocr, "SELECT MAX(timestamp) FROM ocr_results", "SELECT COUNT(*) FROM ocr_results WHERE timestamp >= strftime('%s','now') * 1000 - 3600000"),
        ("screen_frames", config.capture_channels.screen_frames, "SELECT MAX(timestamp) FROM frames", "SELECT COUNT(*) FROM frames WHERE timestamp >= strftime('%s','now') * 1000 - 3600000"),
        ("camera_future", config.capture_channels.camera_future, "SELECT MAX(value) FROM (SELECT MAX(timestamp) AS value FROM visual_scene_snapshots UNION ALL SELECT MAX(timestamp) AS value FROM gaze_samples UNION ALL SELECT MAX(timestamp) AS value FROM attention_snapshots UNION ALL SELECT MAX(last_seen_at) AS value FROM attention_spans)", "SELECT (SELECT COUNT(*) FROM visual_scene_snapshots WHERE timestamp >= strftime('%s','now') * 1000 - 3600000) + (SELECT COUNT(*) FROM gaze_samples WHERE timestamp >= strftime('%s','now') * 1000 - 3600000) + (SELECT COUNT(*) FROM attention_snapshots WHERE timestamp >= strftime('%s','now') * 1000 - 3600000) + (SELECT COUNT(*) FROM attention_spans WHERE last_seen_at >= strftime('%s','now') * 1000 - 3600000)"),
        ("audio_future", config.capture_channels.audio_future, "SELECT MAX(start_timestamp) FROM audio_chunks", "SELECT COUNT(*) FROM audio_chunks WHERE start_timestamp >= strftime('%s','now') * 1000 - 3600000"),
    ];

    let mut statuses = Vec::new();
    for (channel, enabled, last_query, count_query) in channels {
        let permission_state =
            channel_permission_state_for_config(&state.consent_manager, channel, &config).await;
        if !enabled {
            statuses.push(ChannelStatusDto {
                channel: channel.to_string(),
                enabled: false,
                health: "off".to_string(),
                permission_state,
                last_event_time: None,
                sample_count: 0,
                throughput_per_minute: 0.0,
                last_error: None,
                supports_solo_test: !matches!(channel, "ocr"),
                details: "This channel is off, so SOURCE will not collect new samples from it."
                    .to_string(),
            });
            continue;
        }
        let last_event_time =
            context_timeline::get_last_event_time_for_table(&state.db, last_query)
                .await
                .map_err(|e| format!("Failed to inspect channel status: {}", e))?;
        let sample_count = context_timeline::get_count_for_query(&state.db, count_query)
            .await
            .map_err(|e| format!("Failed to inspect channel count: {}", e))?;
        let details = match channel {
            "visible_windows" => "Best-effort running-app snapshots, not a full historical window server feed.",
            "ocr" => "OCR review stays available when text exists, but live OCR ingestion is still intentionally degradable in v1.",
            "camera_future" => "Vision scenes use local camera sampling with MediaPipe pose and face/iris analysis. Gaze and attention only promote when an active eye-tracking calibration exists.",
            "audio_future" => "Audio state can record a selected microphone and mixed desktop/app output as separate sources for VAD, ASR, emotion, and sound-event detection.",
            _ => "Ready for independent channel validation.",
        };
        statuses.push(ChannelStatusDto {
            channel: channel.to_string(),
            enabled,
            health: if runtime.channel_errors.contains_key(channel) || permission_state == "missing"
            {
                "degraded".to_string()
            } else if last_event_time.is_some() {
                "healthy".to_string()
            } else if runtime.is_active {
                "warming_up".to_string()
            } else {
                "idle".to_string()
            },
            permission_state,
            last_event_time,
            sample_count,
            throughput_per_minute: sample_count as f32 / 60.0,
            last_error: runtime.channel_errors.get(channel).cloned(),
            supports_solo_test: !matches!(channel, "ocr"),
            details: details.to_string(),
        });
    }
    Ok(statuses)
}

async fn channel_permission_state_for_config(
    consent_manager: &Arc<ConsentManager>,
    channel: &str,
    config: &Config,
) -> String {
    if channel != "audio_future" {
        return channel_permission_state(consent_manager, channel).await;
    }

    let mut missing = false;
    let mut checked = false;
    if config.audio_microphone_enabled {
        checked = true;
        missing |= !matches!(
            consent_manager
                .is_consent_granted(Feature::MicrophoneRecording)
                .await,
            Ok(true)
        );
    }
    if config.audio_desktop_enabled {
        checked = true;
        missing |= !matches!(
            consent_manager
                .is_consent_granted(Feature::ScreenRecording)
                .await,
            Ok(true)
        );
    }

    if !checked {
        "not_required".to_string()
    } else if missing {
        "missing".to_string()
    } else {
        "granted".to_string()
    }
}
