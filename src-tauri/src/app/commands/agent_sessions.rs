use crate::core::agent_sessions::AgentSessionsSnapshot;

/// Sessions from Codex, Claude Code, Factory and OpenCode, newest first, plus
/// any app the hub could not read.
///
/// Read-only: the agent apps are never started or written to, so this is safe
/// to call while they are running.
#[tauri::command]
pub async fn list_agent_sessions(limit: Option<usize>) -> Result<AgentSessionsSnapshot, String> {
    let limit = limit.unwrap_or(40).clamp(1, 500);
    Ok(crate::core::agent_sessions::list_agent_sessions(limit).await)
}
