use crate::core::agent_bridge::AgentBridge;
use crate::core::agent_sessions::{watch_agent_changes, AgentRoots};
use crate::core::config::Config;
use crate::core::mobile::{AgentServices, PhonePromptSetting};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::watch;

/// Start sending prompts into agent conversations, and tell the window and the
/// phone when sessions change or a conversation SOURCE is driving reports
/// something.
///
/// Both feeds are event-driven: file change notifications for the session
/// list, and the bridge's own broadcast for turns. Returns what the phone
/// server needs to offer the same to a paired phone.
pub fn start_agent_bridge(app: &AppHandle, config: Arc<Mutex<Config>>) -> AgentServices {
    let roots = AgentRoots::from_env();
    let bridge = AgentBridge::new(roots.clone(), config.clone());
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

    // One file watcher feeds both the window and the phone.
    let (changes_tx, changes_rx) = watch::channel(0u64);
    let handle = app.clone();
    let watch_roots = roots.clone();
    tauri::async_runtime::spawn(async move {
        let (watch, mut changes) = match watch_agent_changes(&watch_roots) {
            Ok(started) => started,
            Err(error) => {
                eprintln!("[agents] {error}");
                return;
            }
        };
        while changes.changed().await.is_ok() {
            changes_tx.send_modify(|count| *count += 1);
            let _ = handle.emit("agent-sessions-changed", ());
        }
        drop(watch);
    });

    let enabled = config.lock().map(|config| config.mobile_agent_prompts_enabled).unwrap_or(false);
    let (setting, can_send_changes) = PhonePromptSetting::new(enabled);
    app.manage(setting);

    AgentServices { bridge, changes: changes_rx, can_send_changes, config, roots }
}
