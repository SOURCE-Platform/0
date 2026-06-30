use crate::app::state::{AppState, RecordingInfo};
use crate::core::playback_engine::{PlaybackInfo, SeekInfo};
use tauri::State;
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyboardEventDto {
    id: String,
    timestamp: i64,
    event_type: String,
    key_char: Option<String>,
    key_code: i64,
    modifiers: ModifierDto,
    app_name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ModifierDto {
    ctrl: bool,
    shift: bool,
    alt: bool,
    meta: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MouseEventDto {
    id: String,
    timestamp: i64,
    event_type: String,
    position: PositionDto,
    button: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PositionDto {
    x: i64,
    y: i64,
}

#[derive(sqlx::FromRow)]
struct KeyboardEventRow {
    id: String,
    timestamp: i64,
    event_type: String,
    key_char: Option<String>,
    key_code: i64,
    modifiers_ctrl: bool,
    modifiers_shift: bool,
    modifiers_alt: bool,
    modifiers_meta: bool,
    app_name: String,
}

#[derive(sqlx::FromRow)]
struct MouseEventRow {
    id: String,
    timestamp: i64,
    event_type: String,
    position_x: i64,
    position_y: i64,
    button: Option<String>,
}

#[tauri::command]
pub async fn get_keyboard_events_in_range(
    session_id: String,
    start_time: i64,
    end_time: i64,
    state: State<'_, AppState>,
) -> Result<Vec<KeyboardEventDto>, String> {
    let rows = sqlx::query_as::<_, KeyboardEventRow>(
        r#"
        SELECT id, timestamp, event_type, key_char, key_code,
               modifiers_ctrl, modifiers_shift, modifiers_alt, modifiers_meta, app_name
        FROM keyboard_events
        WHERE session_id = ? AND timestamp >= ? AND timestamp <= ?
        ORDER BY timestamp ASC
        LIMIT 100
        "#,
    )
    .bind(&session_id)
    .bind(start_time)
    .bind(end_time)
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| format!("Failed to get keyboard events: {}", e))?;

    Ok(rows
        .into_iter()
        .map(|row| KeyboardEventDto {
            id: row.id,
            timestamp: row.timestamp,
            event_type: row.event_type,
            key_char: row.key_char,
            key_code: row.key_code,
            modifiers: ModifierDto {
                ctrl: row.modifiers_ctrl,
                shift: row.modifiers_shift,
                alt: row.modifiers_alt,
                meta: row.modifiers_meta,
            },
            app_name: row.app_name,
        })
        .collect())
}

#[tauri::command]
pub async fn get_mouse_events_in_range(
    session_id: String,
    start_time: i64,
    end_time: i64,
    state: State<'_, AppState>,
) -> Result<Vec<MouseEventDto>, String> {
    let rows = sqlx::query_as::<_, MouseEventRow>(
        r#"
        SELECT id, timestamp, event_type, position_x, position_y, button
        FROM mouse_events
        WHERE session_id = ? AND timestamp >= ? AND timestamp <= ?
        ORDER BY timestamp ASC
        LIMIT 100
        "#,
    )
    .bind(&session_id)
    .bind(start_time)
    .bind(end_time)
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| format!("Failed to get mouse events: {}", e))?;

    Ok(rows
        .into_iter()
        .map(|row| MouseEventDto {
            id: row.id,
            timestamp: row.timestamp,
            event_type: row.event_type,
            position: PositionDto {
                x: row.position_x,
                y: row.position_y,
            },
            button: row.button,
        })
        .collect())
}

#[tauri::command]
pub async fn get_playback_info(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<PlaybackInfo, String> {
    let engine = state
        .playback_engine
        .as_ref()
        .ok_or("Playback engine not initialized")?;
    let uuid = Uuid::parse_str(&session_id).map_err(|e| format!("Invalid session ID: {}", e))?;
    engine
        .get_playback_info(uuid)
        .await
        .map_err(|e| format!("Failed to get playback info: {}", e))
}

#[tauri::command]
pub async fn seek_to_timestamp(
    session_id: String,
    timestamp: i64,
    state: State<'_, AppState>,
) -> Result<SeekInfo, String> {
    let engine = state
        .playback_engine
        .as_ref()
        .ok_or("Playback engine not initialized")?;
    let uuid = Uuid::parse_str(&session_id).map_err(|e| format!("Invalid session ID: {}", e))?;
    engine
        .seek_to_timestamp(uuid, timestamp)
        .await
        .map_err(|e| format!("Failed to seek: {}", e))
}

#[tauri::command]
pub async fn get_frame_at_timestamp(
    session_id: String,
    timestamp: i64,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let engine = state
        .playback_engine
        .as_ref()
        .ok_or("Playback engine not initialized")?;
    let uuid = Uuid::parse_str(&session_id).map_err(|e| format!("Invalid session ID: {}", e))?;
    engine
        .get_frame_at_timestamp(uuid, timestamp)
        .await
        .map_err(|e| format!("Failed to get frame: {}", e))
}

#[tauri::command]
pub async fn get_recordings(state: State<'_, AppState>) -> Result<Vec<RecordingInfo>, String> {
    sqlx::query_as::<_, RecordingInfo>(
        r#"SELECT s.id as session_id, s.start_timestamp, s.end_timestamp,
                  COUNT(vs.id) as segment_count,
                  COALESCE(SUM(vs.file_size_bytes), 0) as total_size_bytes,
                  (COALESCE(s.end_timestamp, strftime('%s', 'now')) - s.start_timestamp) * 1000 as total_duration_ms,
                  COALESCE(SUM(vs.frame_count), 0) as frame_count
           FROM sessions s
           INNER JOIN video_segments vs ON vs.session_id = s.id
           GROUP BY s.id
           ORDER BY s.start_timestamp DESC"#,
    )
    .fetch_all(state.db.pool())
    .await
    .map_err(|e| format!("Failed to get recordings: {}", e))
}

#[tauri::command]
pub async fn delete_recording(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let storage = state.storage.as_ref().ok_or("Storage not initialized")?;
    let uuid = Uuid::parse_str(&session_id).map_err(|e| format!("Invalid session ID: {}", e))?;
    storage
        .delete_session(uuid)
        .await
        .map_err(|e| format!("Failed to delete recording: {}", e))
}
