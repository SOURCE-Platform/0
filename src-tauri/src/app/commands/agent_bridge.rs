use crate::core::agent_bridge::{AgentBridge, DriverEvent, SendError};
use std::sync::Arc;
use tauri::State;

/// Send a prompt into a Claude Code conversation as the user's own message.
///
/// Takes the conversation over from the Claude app first when that's safe; the
/// error says why when it isn't. Progress arrives as `agent-turn-event`.
#[tauri::command]
pub async fn agent_send_prompt(
    session_id: String,
    text: String,
    bridge: State<'_, Arc<AgentBridge>>,
) -> Result<DriverEvent, SendError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(SendError::Driver { message: "Nothing to send.".to_string() });
    }
    bridge.inner().send_prompt(&session_id, text).await
}

/// Hand a conversation back to the Claude app now instead of after the quiet period.
#[tauri::command]
pub async fn agent_release_session(session_id: String, bridge: State<'_, Arc<AgentBridge>>) -> Result<(), String> {
    bridge.release(&session_id).await;
    Ok(())
}

/// Conversations SOURCE is currently holding.
#[tauri::command]
pub async fn agent_held_sessions(bridge: State<'_, Arc<AgentBridge>>) -> Result<Vec<String>, String> {
    Ok(bridge.held_sessions().await)
}
