use crate::app::capture::data::{
    actual_recordings_path, clear_screen_evidence_channel, collect_capture_data_overview,
    delete_all_capture_data, ensure_capture_stopped,
};
use crate::app::capture::preview::{build_capture_preview, get_capture_channel_preview_rows};
use crate::app::state::{AppState, CaptureDataOverviewDto, CapturePreviewDto};
use crate::core::database::Database;
use crate::core::gaze;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use tauri::State;

#[tauri::command]
pub async fn get_capture_data_overview(
    state: State<'_, AppState>,
) -> Result<CaptureDataOverviewDto, String> {
    collect_capture_data_overview(&state).await
}

#[tauri::command]
pub async fn reveal_capture_data_target(
    target: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let target_path = match target.as_str() {
        "database" => Database::database_path()
            .map_err(|e| format!("Failed to resolve database path: {}", e))?,
        "recordings" => actual_recordings_path(&state)?,
        "configured_storage" => PathBuf::from(
            state
                .config
                .lock()
                .map_err(|e| format!("Failed to lock config: {}", e))?
                .storage_path
                .clone(),
        ),
        _ => return Err(format!("Unknown reveal target: {}", target)),
    };
    let mut command = ProcessCommand::new("open");
    if target == "database" {
        command.arg("-R").arg(&target_path);
    } else {
        command.arg(&target_path);
    }
    let status = command
        .status()
        .map_err(|e| format!("Failed to reveal path: {}", e))?;
    if status.success() {
        Ok(())
    } else {
        Err("Reveal command exited unsuccessfully.".to_string())
    }
}

#[tauri::command]
pub async fn delete_capture_data(
    channel: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    ensure_capture_stopped(&state).await?;
    match channel.as_deref() {
        Some("system") => {
            delete_single(
                "DELETE FROM context_events WHERE channel = 'system'",
                "system events",
                &state,
            )
            .await?
        }
        Some("focus") => {
            delete_single(
                "DELETE FROM context_events WHERE channel = 'focus'",
                "focus events",
                &state,
            )
            .await?
        }
        Some("visible_windows") => {
            delete_single(
                "DELETE FROM window_snapshots",
                "visible window snapshots",
                &state,
            )
            .await?
        }
        Some("keyboard") => {
            delete_single("DELETE FROM keyboard_events", "keyboard events", &state).await?
        }
        Some("mouse") => delete_single("DELETE FROM mouse_events", "mouse events", &state).await?,
        Some("ocr") => delete_ocr_data(&state).await?,
        Some("screen_frames") => clear_screen_evidence_channel(&state).await?,
        Some("camera_future") => {
            delete_many(
                &[
                    "DELETE FROM visual_state_spans",
                    "DELETE FROM visual_scene_snapshots",
                    "DELETE FROM vision_detections",
                    "DELETE FROM video_frame_samples",
                ],
                "vision data",
                &state,
            )
            .await?;
            gaze::delete_all_gaze_data(&state.db).await?;
        }
        Some("audio_future") => {
            delete_many(
                &[
                    "DELETE FROM audio_state_spans",
                    "DELETE FROM asr_segments",
                    "DELETE FROM audio_chunks",
                ],
                "audio data",
                &state,
            )
            .await?
        }
        None => return delete_all_capture_data(&state).await,
        Some(other) => return Err(format!("Unknown channel: {}", other)),
    }

    sqlx::query("VACUUM")
        .execute(state.db.pool())
        .await
        .map_err(|e| format!("Failed to compact database: {}", e))?;
    Ok(())
}

async fn delete_single(query: &str, label: &str, state: &AppState) -> Result<(), String> {
    sqlx::query(query)
        .execute(state.db.pool())
        .await
        .map_err(|e| format!("Failed to delete {}: {}", label, e))?;
    Ok(())
}

async fn delete_many(queries: &[&str], label: &str, state: &AppState) -> Result<(), String> {
    for query in queries {
        sqlx::query(query)
            .execute(state.db.pool())
            .await
            .map_err(|e| format!("Failed to delete {}: {}", label, e))?;
    }
    Ok(())
}

async fn delete_ocr_data(state: &AppState) -> Result<(), String> {
    delete_single("DELETE FROM ocr_results", "OCR results", state).await?;
    crate::core::ocr_agent_context::delete_all_derived(&state.db)
        .await
        .map_err(|e| format!("Failed to delete derived OCR data: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn get_capture_channel_preview(
    channel: String,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> Result<CapturePreviewDto, String> {
    let label = match channel.as_str() {
        "system" => "OS / session events",
        "focus" => "Focus and running apps",
        "visible_windows" => "Visible windows snapshots",
        "keyboard" => "Keyboard activity",
        "mouse" => "Mouse activity",
        "ocr" => "OCR text capture",
        "screen_frames" => "Screen keyframes / evidence",
        "camera_future" => "Vision / scene",
        "audio_future" => "Audio / speech",
        _ => channel.as_str(),
    }
    .to_string();
    let rows =
        get_capture_channel_preview_rows(&channel, limit.unwrap_or(20).clamp(1, 100), &state)
            .await?;
    Ok(build_capture_preview(channel, label, rows))
}
