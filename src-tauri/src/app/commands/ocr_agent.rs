use crate::app::state::AppState;
use crate::core::ocr_agent_context::{
    self, ActivityEpisodeDto, AgentContextEntityDto, AgentContextSearchResultDto,
    AgentSceneSnapshotDto, AgentTextSpanDto, OcrAgentSummaryDto,
};
use tauri::State;

#[tauri::command]
pub async fn get_scene_snapshot(
    scene_id: String,
    state: State<'_, AppState>,
) -> Result<Option<AgentSceneSnapshotDto>, String> {
    ocr_agent_context::get_scene_snapshot(&state.db, &scene_id)
        .await
        .map_err(|e| format!("Failed to get OCR scene snapshot: {}", e))
}

#[tauri::command]
pub async fn get_scene_snapshots(
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AgentSceneSnapshotDto>, String> {
    ocr_agent_context::get_scene_snapshots(&state.db, start_timestamp, end_timestamp, app_filter)
        .await
        .map_err(|e| format!("Failed to get OCR scene snapshots: {}", e))
}

#[tauri::command]
pub async fn get_text_spans(
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AgentTextSpanDto>, String> {
    ocr_agent_context::get_text_spans(&state.db, start_timestamp, end_timestamp, app_filter)
        .await
        .map_err(|e| format!("Failed to get OCR text spans: {}", e))
}

#[tauri::command]
pub async fn get_context_entities(
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
    entity_type: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AgentContextEntityDto>, String> {
    ocr_agent_context::get_context_entities(
        &state.db,
        start_timestamp,
        end_timestamp,
        app_filter,
        entity_type,
    )
    .await
    .map_err(|e| format!("Failed to get OCR context entities: {}", e))
}

#[tauri::command]
pub async fn search_agent_context(
    query: String,
    start_timestamp: Option<i64>,
    end_timestamp: Option<i64>,
    app_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AgentContextSearchResultDto>, String> {
    ocr_agent_context::search_agent_context(
        &state.db,
        &query,
        start_timestamp,
        end_timestamp,
        app_filter,
    )
    .await
    .map_err(|e| format!("Failed to search OCR agent context: {}", e))
}

#[tauri::command]
pub async fn get_activity_episode(
    timestamp: i64,
    state: State<'_, AppState>,
) -> Result<ActivityEpisodeDto, String> {
    ocr_agent_context::get_activity_episode(&state.db, timestamp)
        .await
        .map_err(|e| format!("Failed to get OCR activity episode: {}", e))
}

#[tauri::command]
pub async fn get_ocr_agent_summary(
    start_timestamp: i64,
    end_timestamp: i64,
    state: State<'_, AppState>,
) -> Result<OcrAgentSummaryDto, String> {
    ocr_agent_context::get_ocr_agent_summary(&state.db, start_timestamp, end_timestamp)
        .await
        .map_err(|e| format!("Failed to get OCR agent summary: {}", e))
}
