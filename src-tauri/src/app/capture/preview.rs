use crate::app::state::{AppState, CapturePreviewDto, CapturePreviewRowDto};
use sqlx::Row;

#[path = "preview_media.rs"]
mod preview_media;

pub(super) fn pretty_json(value: serde_json::Value) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

pub async fn get_capture_channel_preview_rows(
    channel: &str,
    limit: i64,
    state: &AppState,
) -> Result<Vec<CapturePreviewRowDto>, String> {
    match channel {
        "system" | "focus" => preview_context_events(channel, limit, state).await,
        "visible_windows" => preview_visible_windows(limit, state).await,
        "keyboard" => preview_keyboard(limit, state).await,
        "mouse" => preview_mouse(limit, state).await,
        "ocr" => preview_ocr(limit, state).await,
        "screen_frames" => preview_frames(limit, state).await,
        "camera_future" => preview_media::preview_visual(limit, state).await,
        "audio_future" => preview_media::preview_audio(limit, state).await,
        other => Err(format!("Unknown channel: {}", other)),
    }
}

pub fn build_capture_preview(
    channel: String,
    label: String,
    rows: Vec<CapturePreviewRowDto>,
) -> CapturePreviewDto {
    CapturePreviewDto {
        channel,
        label,
        rows,
    }
}

async fn preview_context_events(
    channel: &str,
    limit: i64,
    state: &AppState,
) -> Result<Vec<CapturePreviewRowDto>, String> {
    let db_rows = sqlx::query(
        "SELECT timestamp, event_type, source, confidence, payload_json
         FROM context_events
         WHERE channel = ?
         ORDER BY timestamp DESC
         LIMIT ?",
    )
    .bind(channel)
    .bind(limit)
    .fetch_all(state.db.pool())
    .await
    .map_err(|e| format!("Failed to load {} preview: {}", channel, e))?;

    Ok(db_rows
        .into_iter()
        .map(|row| {
            let timestamp = row.get::<i64, _>("timestamp");
            let raw = serde_json::json!({
                "timestamp": timestamp,
                "event_type": row.get::<String, _>("event_type"),
                "source": row.get::<String, _>("source"),
                "confidence": row.get::<f64, _>("confidence"),
                "payload_json": row.get::<Option<String>, _>("payload_json"),
            });
            CapturePreviewRowDto {
                timestamp: Some(timestamp),
                summary: format!(
                    "{} via {}",
                    row.get::<String, _>("event_type"),
                    row.get::<String, _>("source")
                ),
                raw_json: pretty_json(raw),
            }
        })
        .collect())
}

async fn preview_visible_windows(
    limit: i64,
    state: &AppState,
) -> Result<Vec<CapturePreviewRowDto>, String> {
    let db_rows = sqlx::query(
        "SELECT timestamp, frontmost_app_name, frontmost_bundle_id, visible_windows_json, confidence
         FROM window_snapshots
         ORDER BY timestamp DESC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(state.db.pool())
    .await
    .map_err(|e| format!("Failed to load visible windows preview: {}", e))?;

    Ok(db_rows
        .into_iter()
        .map(|row| {
            let timestamp = row.get::<i64, _>("timestamp");
            let app_name = row.get::<Option<String>, _>("frontmost_app_name");
            let raw = serde_json::json!({
                "timestamp": timestamp,
                "frontmost_app_name": app_name,
                "frontmost_bundle_id": row.get::<Option<String>, _>("frontmost_bundle_id"),
                "confidence": row.get::<f64, _>("confidence"),
                "visible_windows": serde_json::from_str::<serde_json::Value>(&row.get::<String, _>("visible_windows_json")).unwrap_or(serde_json::Value::Null),
            });
            CapturePreviewRowDto {
                timestamp: Some(timestamp),
                summary: format!("Frontmost app: {}", app_name.unwrap_or_else(|| "Unknown".to_string())),
                raw_json: pretty_json(raw),
            }
        })
        .collect())
}

async fn preview_keyboard(
    limit: i64,
    state: &AppState,
) -> Result<Vec<CapturePreviewRowDto>, String> {
    let db_rows = sqlx::query(
        "SELECT timestamp, event_type, key_code, key_char, modifiers, app_name, window_title, process_id, ui_element
         FROM keyboard_events
         ORDER BY timestamp DESC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(state.db.pool())
    .await
    .map_err(|e| format!("Failed to load keyboard preview: {}", e))?;

    Ok(db_rows
        .into_iter()
        .map(|row| {
            let timestamp = row.get::<i64, _>("timestamp");
            let app_name = row.get::<String, _>("app_name");
            let event_type = row.get::<String, _>("event_type");
            let raw = serde_json::json!({
                "timestamp": timestamp,
                "event_type": event_type,
                "key_code": row.get::<i64, _>("key_code"),
                "key_char": row.get::<Option<String>, _>("key_char"),
                "modifiers": serde_json::from_str::<serde_json::Value>(&row.get::<String, _>("modifiers")).unwrap_or(serde_json::Value::Null),
                "app_name": app_name,
                "window_title": row.get::<String, _>("window_title"),
                "process_id": row.get::<i64, _>("process_id"),
                "ui_element": row.get::<Option<String>, _>("ui_element").and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok()).unwrap_or(serde_json::Value::Null),
            });
            CapturePreviewRowDto {
                timestamp: Some(timestamp),
                summary: format!("{event_type} in {app_name}"),
                raw_json: pretty_json(raw),
            }
        })
        .collect())
}

async fn preview_mouse(limit: i64, state: &AppState) -> Result<Vec<CapturePreviewRowDto>, String> {
    let db_rows = sqlx::query(
        "SELECT timestamp, event_type, position_x, position_y, app_name, window_title, process_id, ui_element
         FROM mouse_events
         ORDER BY timestamp DESC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(state.db.pool())
    .await
    .map_err(|e| format!("Failed to load mouse preview: {}", e))?;

    Ok(db_rows
        .into_iter()
        .map(|row| {
            let timestamp = row.get::<i64, _>("timestamp");
            let event_type = row.get::<String, _>("event_type");
            let x = row.get::<i64, _>("position_x");
            let y = row.get::<i64, _>("position_y");
            let app_name = row.get::<String, _>("app_name");
            let raw = serde_json::json!({
                "timestamp": timestamp,
                "event_type": event_type,
                "position_x": x,
                "position_y": y,
                "app_name": app_name,
                "window_title": row.get::<String, _>("window_title"),
                "process_id": row.get::<i64, _>("process_id"),
                "ui_element": row.get::<Option<String>, _>("ui_element").and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok()).unwrap_or(serde_json::Value::Null),
            });
            CapturePreviewRowDto {
                timestamp: Some(timestamp),
                summary: format!("{event_type} at ({x}, {y}) in {app_name}"),
                raw_json: pretty_json(raw),
            }
        })
        .collect())
}

async fn preview_ocr(limit: i64, state: &AppState) -> Result<Vec<CapturePreviewRowDto>, String> {
    let db_rows = sqlx::query(
        "SELECT timestamp, text, confidence, frame_path, bounding_box, language, processing_time_ms
         FROM ocr_results
         ORDER BY timestamp DESC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(state.db.pool())
    .await
    .map_err(|e| format!("Failed to load OCR preview: {}", e))?;

    Ok(db_rows
        .into_iter()
        .map(|row| {
            let timestamp = row.get::<i64, _>("timestamp");
            let text = row.get::<String, _>("text");
            let preview = text.chars().take(90).collect::<String>();
            let raw = serde_json::json!({
                "timestamp": timestamp,
                "text": text,
                "confidence": row.get::<f64, _>("confidence"),
                "frame_path": row.get::<Option<String>, _>("frame_path"),
                "bounding_box": serde_json::from_str::<serde_json::Value>(&row.get::<String, _>("bounding_box")).unwrap_or(serde_json::Value::Null),
                "language": row.get::<String, _>("language"),
                "processing_time_ms": row.get::<Option<i64>, _>("processing_time_ms"),
            });
            CapturePreviewRowDto {
                timestamp: Some(timestamp),
                summary: preview,
                raw_json: pretty_json(raw),
            }
        })
        .collect())
}

async fn preview_frames(limit: i64, state: &AppState) -> Result<Vec<CapturePreviewRowDto>, String> {
    let db_rows = sqlx::query(
        "SELECT timestamp, file_path, width, height
         FROM frames
         ORDER BY timestamp DESC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(state.db.pool())
    .await
    .map_err(|e| format!("Failed to load frame preview: {}", e))?;

    Ok(db_rows
        .into_iter()
        .map(|row| {
            let timestamp = row.get::<i64, _>("timestamp");
            let width = row.get::<i64, _>("width");
            let height = row.get::<i64, _>("height");
            let raw = serde_json::json!({
                "timestamp": timestamp,
                "file_path": row.get::<String, _>("file_path"),
                "width": width,
                "height": height,
            });
            CapturePreviewRowDto {
                timestamp: Some(timestamp),
                summary: format!("{width}×{height} frame"),
                raw_json: pretty_json(raw),
            }
        })
        .collect())
}
