use crate::app::state::AppState;
use crate::core::command_analyzer::{CommandAnalyzer, CommandStats};
use crate::core::search_engine::{SearchFilters, SearchQuery, SearchResults};
use tauri::State;
use uuid::Uuid;

#[tauri::command]
pub async fn get_most_used_shortcuts(
    limit: u32,
    state: State<'_, AppState>,
) -> Result<Vec<(String, u32)>, String> {
    let stats = get_command_stats(None, state).await?;
    Ok(stats
        .most_used_shortcuts
        .into_iter()
        .take(limit as usize)
        .collect())
}

#[tauri::command]
pub async fn search_text(
    query: String,
    filters: SearchFilters,
    limit: u32,
    offset: u32,
    state: State<'_, AppState>,
) -> Result<SearchResults, String> {
    state
        .search_engine
        .search(SearchQuery {
            query,
            filters,
            limit,
            offset,
        })
        .await
        .map_err(|e| format!("Search failed: {}", e))
}

#[tauri::command]
pub async fn search_suggestions(
    partial: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    state
        .search_engine
        .suggest_queries(&partial)
        .await
        .map_err(|e| format!("Failed to get suggestions: {}", e))
}

#[tauri::command]
pub async fn search_in_session(
    session_id: String,
    query: String,
    state: State<'_, AppState>,
) -> Result<SearchResults, String> {
    let session_uuid =
        Uuid::parse_str(&session_id).map_err(|e| format!("Invalid session ID: {}", e))?;
    state
        .search_engine
        .search(SearchQuery {
            query,
            filters: SearchFilters {
                session_ids: Some(vec![session_uuid]),
                ..Default::default()
            },
            limit: 50,
            offset: 0,
        })
        .await
        .map_err(|e| format!("Search failed: {}", e))
}

#[tauri::command]
pub async fn get_command_stats(
    session_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<CommandStats, String> {
    let db = &state.db;
    let session_uuid = if let Some(sid) = session_id {
        Some(Uuid::parse_str(&sid).map_err(|e| format!("Invalid session ID: {}", e))?)
    } else {
        None
    };
    CommandAnalyzer::get_command_stats(db, session_uuid)
        .await
        .map_err(|e| format!("Failed to get command stats: {}", e))
}
