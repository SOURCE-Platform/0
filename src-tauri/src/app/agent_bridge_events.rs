use crate::core::agent_bridge::AgentBridge;
use crate::core::agent_sessions::{watch_agent_changes, AgentRoots};
use tauri::{AppHandle, Emitter, Manager};

/// Start sending prompts into agent conversations, and tell the window when
/// sessions change or a conversation SOURCE is driving reports something.
///
/// Both feeds are event-driven: file change notifications for the session
/// list, and the bridge's own broadcast for turns.
pub fn start_agent_bridge(app: &AppHandle) {
    let roots = AgentRoots::from_env();
    let bridge = AgentBridge::new(roots.clone());
    app.manage(bridge.clone());

    let handle = app.clone();
    let mut turns = bridge.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match turns.recv().await {
                Ok(event) => {
                    let _ = handle.emit("agent-turn-event", event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let (watch, mut changes) = match watch_agent_changes(&roots) {
            Ok(started) => started,
            Err(error) => {
                eprintln!("[agents] {error}");
                return;
            }
        };
        while changes.changed().await.is_ok() {
            let _ = handle.emit("agent-sessions-changed", ());
        }
        drop(watch);
    });
}
