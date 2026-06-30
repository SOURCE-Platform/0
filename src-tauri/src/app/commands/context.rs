use crate::app::state::AppState;
use crate::core::context_timeline::{
    self, AppUsageOverviewDto, ContextInspectorDto, ContextSliceDetailDto,
    ContextTimelineData as DesktopContextTimelineData, OcrReviewItemDto, PiiEntityDto,
};
use tauri::State;

#[tauri::command]
pub async fn get_context_timeline(
    start_timestamp: i64,
    end_timestamp: i64,
    state: State<'_, AppState>,
) -> Result<DesktopContextTimelineData, String> {
    context_timeline::build_context_timeline(&state.db, start_timestamp, end_timestamp)
        .await
        .map_err(|e| format!("Failed to build context timeline: {}", e))
}

#[tauri::command]
pub async fn get_context_inspector(
    timestamp: i64,
    state: State<'_, AppState>,
) -> Result<ContextInspectorDto, String> {
    context_timeline::get_context_inspector(&state.db, timestamp)
        .await
        .map_err(|e| format!("Failed to build context inspector: {}", e))
}

#[tauri::command]
pub async fn get_context_slice_detail(
    slice_id: String,
    rail_id: String,
    start_timestamp: i64,
    end_timestamp: i64,
    state: State<'_, AppState>,
) -> Result<ContextSliceDetailDto, String> {
    context_timeline::get_context_slice_detail(
        &state.db,
        &slice_id,
        &rail_id,
        start_timestamp,
        end_timestamp,
    )
    .await
    .map_err(|e| format!("Failed to build context slice detail: {}", e))
}

#[tauri::command]
pub async fn get_app_usage_overview(
    start_timestamp: i64,
    end_timestamp: i64,
    state: State<'_, AppState>,
) -> Result<AppUsageOverviewDto, String> {
    context_timeline::get_app_usage_overview(&state.db, start_timestamp, end_timestamp)
        .await
        .map_err(|e| format!("Failed to get app usage overview: {}", e))
}

#[tauri::command]
pub async fn get_pii_review(
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
    entity_type_filter: Option<String>,
    confidence_threshold: Option<f32>,
    state: State<'_, AppState>,
) -> Result<Vec<PiiEntityDto>, String> {
    context_timeline::get_pii_review(
        &state.db,
        start_timestamp,
        end_timestamp,
        app_filter,
        entity_type_filter,
        confidence_threshold,
    )
    .await
    .map_err(|e| format!("Failed to get PII review: {}", e))
}

#[tauri::command]
pub async fn get_ocr_review(
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
    query: Option<String>,
    pii_only: bool,
    state: State<'_, AppState>,
) -> Result<Vec<OcrReviewItemDto>, String> {
    context_timeline::get_ocr_review(
        &state.db,
        start_timestamp,
        end_timestamp,
        app_filter,
        query,
        pii_only,
    )
    .await
    .map_err(|e| format!("Failed to get OCR review: {}", e))
}
