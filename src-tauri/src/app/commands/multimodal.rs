use crate::app::state::AppState;
use crate::core::multimodal::{
    self, AsrSegmentDto, AudioStateSpanDto, MultimodalActivityEpisodeDto, VisualAudioSummaryDto,
    VisualSceneSnapshotDto, VisualStateSpanDto,
};
use tauri::State;

#[tauri::command]
pub async fn get_visual_scene_snapshot(
    visual_scene_id: String,
    state: State<'_, AppState>,
) -> Result<Option<VisualSceneSnapshotDto>, String> {
    multimodal::get_visual_scene_snapshot(&state.db, &visual_scene_id)
        .await
        .map_err(|e| format!("Failed to get visual scene snapshot: {}", e))
}

#[tauri::command]
pub async fn get_visual_scene_snapshots(
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<VisualSceneSnapshotDto>, String> {
    multimodal::get_visual_scene_snapshots(&state.db, start_timestamp, end_timestamp, source_filter)
        .await
        .map_err(|e| format!("Failed to get visual scene snapshots: {}", e))
}

#[tauri::command]
pub async fn get_visual_state_spans(
    start_timestamp: i64,
    end_timestamp: i64,
    state_type: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<VisualStateSpanDto>, String> {
    multimodal::get_visual_state_spans(&state.db, start_timestamp, end_timestamp, state_type)
        .await
        .map_err(|e| format!("Failed to get visual state spans: {}", e))
}

#[tauri::command]
pub async fn get_audio_state_spans(
    start_timestamp: i64,
    end_timestamp: i64,
    state: State<'_, AppState>,
) -> Result<Vec<AudioStateSpanDto>, String> {
    multimodal::get_audio_state_spans(&state.db, start_timestamp, end_timestamp)
        .await
        .map_err(|e| format!("Failed to get audio state spans: {}", e))
}

#[tauri::command]
pub async fn get_asr_segments(
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AsrSegmentDto>, String> {
    multimodal::get_asr_segments(&state.db, start_timestamp, end_timestamp, source_filter)
        .await
        .map_err(|e| format!("Failed to get ASR segments: {}", e))
}

#[tauri::command]
pub async fn get_visual_audio_summary(
    start_timestamp: i64,
    end_timestamp: i64,
    state: State<'_, AppState>,
) -> Result<VisualAudioSummaryDto, String> {
    multimodal::get_visual_audio_summary(&state.db, start_timestamp, end_timestamp)
        .await
        .map_err(|e| format!("Failed to get visual/audio summary: {}", e))
}

#[tauri::command]
pub async fn get_multimodal_activity_episode(
    timestamp: i64,
    state: State<'_, AppState>,
) -> Result<MultimodalActivityEpisodeDto, String> {
    multimodal::get_multimodal_activity_episode(&state.db, timestamp)
        .await
        .map_err(|e| format!("Failed to get multimodal activity episode: {}", e))
}
