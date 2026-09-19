use crate::app::commands::*;
use crate::app::setup::setup_app;
use crate::app::tray;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            tray::show_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| setup_app(app))
        .invoke_handler(tauri::generate_handler![
            greet,
            list_agent_sessions,
            agent_session_messages,
            agent_send_prompt,
            agent_release_session,
            agent_held_sessions,
            check_consent_status,
            request_consent,
            revoke_consent,
            get_all_consents,
            get_config,
            update_config,
            reset_config,
            get_available_displays,
            get_host_hardware_info,
            get_dictation_words_visible,
            set_dictation_words_visible,
            start_screen_recording,
            stop_screen_recording,
            get_recording_status,
            start_desktop_capture,
            stop_desktop_capture,
            restart_multimodal_capture,
            get_desktop_capture_status,
            get_channel_statuses,
            open_screen_capture_settings,
            get_capture_data_overview,
            reveal_capture_data_target,
            delete_capture_data,
            get_capture_channel_preview,
            register_capture_excluded_window,
            unregister_capture_excluded_window,
            sensitive_capture_surface_changed,
            capture_suppression_active,
            report_csp_violation,
            webview_smoke_event,
            debug_asset_scope_probe,
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
            get_asr_segment,
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
            delete_recording,
            mobile_pairing_qr,
            mobile_clear_pairing_qr,
            mobile_pending_pair_requests,
            mobile_approve_pair,
            mobile_deny_pair,
            mobile_tls_fingerprint,
            mobile_server_port,
            mobile_list_devices,
            mobile_unpair_device,
            set_mobile_agent_prompts_enabled,
            set_agent_prompts_keep_bypass,
            debug_mobile_transcribe,
            vault_state,
            vault_setup,
            vault_unlock,
            vault_change_master_password,
            vault_lock,
            vault_unlock_with_recovery_key,
            vault_rotate_recovery_key,
            vault_reset_master_password,
            vault_list_items,
            vault_add_login,
            vault_update_item,
            vault_delete_item,
            vault_reveal,
            vault_set_auto_lock_minutes
        ])
        .on_window_event(|window, event| {
            // Standard tray behavior: closing the window hides it to the
            // menu bar instead of quitting. Tray -> Quit is the real exit.
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    tray::hide_main_window(&window.app_handle().clone());
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // §1.6: on the way out, tell the helper to lock (zeroize);
            // dropping the connection then lets it exit by its rules.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Exit = event {
                crate::core::vault_client::shutdown();
            }
        });
}
