use crate::core::agent_sessions::{AgentApp, AgentMessage, AgentRoots, AgentSessionsSnapshot};

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

/// The most recent readable messages of one conversation, oldest first.
///
/// Read-only, like the list: it reads the transcript the agent app keeps.
#[tauri::command]
pub async fn agent_session_messages(
    app: AgentApp,
    session_id: String,
    limit: Option<usize>,
) -> Result<Vec<AgentMessage>, String> {
    let limit = limit.unwrap_or(40).clamp(1, 400);
    match app {
        AgentApp::ClaudeCode => tauri::async_runtime::spawn_blocking(move || {
            crate::core::agent_sessions::claude_recent_messages(&AgentRoots::from_env(), &session_id, limit)
        })
        .await
        .map_err(|error| format!("Reading the conversation failed: {error}"))?,
        other => Err(format!("Reading {} conversations isn't supported yet.", other.label())),
    }
}
