use crate::app::state::AppState;
use crate::core::gaze::{
    self, AttentionAtTimestampDto, AttentionSnapshotDto, AttentionSpanDto, AttentionSummaryDto,
    GazeCalibrationDto, GazeSampleDto,
};
use crate::core::multimodal::service::list_avfoundation_sources;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraSourceDto {
    pub camera_id: String,
    pub name: String,
    pub index: i32,
}

#[tauri::command]
pub async fn start_gaze_calibration(
    display_id: Option<u32>,
    display_name: Option<String>,
    camera_id: Option<String>,
    display_x: i32,
    display_y: i32,
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
    gaze::start_gaze_calibration(
        &state.db,
        session_id,
        display_id,
        display_name,
        camera_id,
        display_x,
        display_y,
        screen_width,
        screen_height,
    )
    .await
}

#[tauri::command]
pub async fn list_gaze_camera_sources() -> Result<Vec<CameraSourceDto>, String> {
    let sources = list_avfoundation_sources().await?;
    Ok(sources
        .video
        .into_iter()
        .map(|source| CameraSourceDto {
            camera_id: format!("camera:{}", source.index),
            name: source.name,
            index: source.index,
        })
        .collect())
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
    display_id: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Option<GazeCalibrationDto>, String> {
    gaze::get_active_gaze_calibration(&state.db, display_id).await
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
