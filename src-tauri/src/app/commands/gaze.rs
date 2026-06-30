use crate::app::state::AppState;
use crate::core::gaze::{
    self, AttentionAtTimestampDto, AttentionSnapshotDto, AttentionSpanDto, AttentionSummaryDto,
    GazeCalibrationDto, GazeSampleDto,
};
use tauri::State;

#[tauri::command]
pub async fn start_gaze_calibration(
    screen_width: i64,
    screen_height: i64,
    state: State<'_, AppState>,
) -> Result<GazeCalibrationDto, String> {
    let session_id = state
        .desktop_capture_runtime
        .read()
        .await
        .session_id
        .clone();
    gaze::start_gaze_calibration(&state.db, session_id, screen_width, screen_height).await
}

#[tauri::command]
pub async fn capture_gaze_calibration_sample(
    calibration_id: String,
    phase: String,
    target_x: f32,
    target_y: f32,
    state: State<'_, AppState>,
) -> Result<GazeCalibrationDto, String> {
    gaze::capture_gaze_calibration_sample(&state.db, &calibration_id, phase, target_x, target_y)
        .await
}

#[tauri::command]
pub async fn finalize_gaze_calibration(
    calibration_id: String,
    state: State<'_, AppState>,
) -> Result<GazeCalibrationDto, String> {
    gaze::finalize_gaze_calibration(&state.db, &calibration_id).await
}

#[tauri::command]
pub async fn get_active_gaze_calibration(
    state: State<'_, AppState>,
) -> Result<Option<GazeCalibrationDto>, String> {
    gaze::get_active_gaze_calibration(&state.db).await
}

#[tauri::command]
pub async fn get_gaze_samples(
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
    session_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<GazeSampleDto>, String> {
    gaze::get_gaze_samples(
        &state.db,
        start_timestamp,
        end_timestamp,
        source_filter,
        session_filter,
    )
    .await
}

#[tauri::command]
pub async fn get_attention_snapshots(
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
    session_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AttentionSnapshotDto>, String> {
    gaze::get_attention_snapshots(
        &state.db,
        start_timestamp,
        end_timestamp,
        source_filter,
        session_filter,
    )
    .await
}

#[tauri::command]
pub async fn get_attention_spans(
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AttentionSpanDto>, String> {
    gaze::get_attention_spans(&state.db, start_timestamp, end_timestamp, source_filter).await
}

#[tauri::command]
pub async fn get_attention_at_timestamp(
    timestamp: i64,
    state: State<'_, AppState>,
) -> Result<AttentionAtTimestampDto, String> {
    gaze::get_attention_at_timestamp(&state.db, timestamp).await
}

#[tauri::command]
pub async fn get_attention_summary(
    start_timestamp: i64,
    end_timestamp: i64,
    state: State<'_, AppState>,
) -> Result<AttentionSummaryDto, String> {
    gaze::get_attention_summary(&state.db, start_timestamp, end_timestamp).await
}

#[tauri::command]
pub async fn search_attention_context(
    query: String,
    start_timestamp: Option<i64>,
    end_timestamp: Option<i64>,
    source_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<gaze::AttentionSearchResultDto>, String> {
    gaze::search_attention_context(
        &state.db,
        query,
        start_timestamp,
        end_timestamp,
        source_filter,
    )
    .await
}
