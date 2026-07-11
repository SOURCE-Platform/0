use crate::app::commands::*;
use crate::app::setup::setup_app;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| setup_app(app))
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
            start_desktop_capture,
            stop_desktop_capture,
            get_desktop_capture_status,
            get_channel_statuses,
            get_capture_data_overview,
            reveal_capture_data_target,
            delete_capture_data,
            get_capture_channel_preview,
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
            get_context_timeline,
            get_context_inspector,
            get_context_slice_detail,
            get_scene_snapshot,
            get_scene_snapshots,
            get_text_spans,
            get_context_entities,
            search_agent_context,
            get_activity_episode,
            get_ocr_agent_summary,
            get_visual_scene_snapshot,
            get_visual_scene_snapshots,
            get_visual_state_spans,
            get_audio_state_spans,
            get_asr_segments,
            get_speech_emotion_segments,
            get_sound_event_detections,
            get_sound_event_spans,
            get_visual_audio_summary,
            get_multimodal_activity_episode,
            list_audio_input_sources,
            get_audio_source_meters,
            start_audio_meter_stream,
            stop_audio_meter_stream,
            start_gaze_calibration,
            list_gaze_camera_sources,
            capture_gaze_calibration_sample,
            finalize_gaze_calibration,
            get_active_gaze_calibration,
            get_gaze_samples,
            get_attention_snapshots,
            get_attention_spans,
            get_attention_at_timestamp,
            get_attention_summary,
            search_attention_context,
            get_app_usage_overview,
            get_pii_review,
            get_ocr_review,
            get_keyboard_events_in_range,
            get_mouse_events_in_range,
            get_playback_info,
            seek_to_timestamp,
            get_frame_at_timestamp,
            get_recordings,
            delete_recording
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
