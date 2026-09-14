//! What the phone's agent connection sees: the session list, the open
//! conversation's messages, and whether it may send prompts.

use crate::core::agent_bridge::AgentBridge;
use crate::core::agent_sessions::{
    claude_recent_messages, list_from, AgentApp, AgentMessage, AgentRoots, AgentSession,
};
use crate::core::config::Config;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::watch;

const SESSION_LIMIT: usize = 40;
const MESSAGE_LIMIT: usize = 60;

/// Agent features the phone server needs, handed over by app setup.
#[derive(Clone)]
pub struct AgentServices {
    pub bridge: Arc<AgentBridge>,
    /// Counts up whenever agent session files change (FSEvents, settled).
    pub changes: watch::Receiver<u64>,
    pub config: Arc<Mutex<Config>>,
    pub roots: AgentRoots,
}

impl AgentServices {
    /// Read live, so turning the setting off takes effect on the next send.
    pub fn phone_may_send(&self) -> bool {
        self.config.lock().map(|config| config.mobile_agent_prompts_enabled).unwrap_or(false)
    }

    pub async fn sessions(&self) -> (Vec<AgentSession>, Vec<String>) {
        let sessions = list_from(&self.roots, SESSION_LIMIT).await.sessions;
        (sessions, self.bridge.held_sessions().await)
    }

    /// Recent messages of one conversation, or `None` when that app's
    /// conversations can't be read yet or the transcript is gone.
    pub async fn messages(&self, app: AgentApp, session_id: &str) -> Option<Vec<AgentMessage>> {
        if app != AgentApp::ClaudeCode {
            return None;
        }
        let roots = self.roots.clone();
        let session_id = session_id.to_string();
        tokio::task::spawn_blocking(move || claude_recent_messages(&roots, &session_id, MESSAGE_LIMIT).ok())
            .await
            .ok()
            .flatten()
    }
}

/// Identifies this run of the Mac app. A phone that sees a new epoch knows its
/// cached state belongs to a previous run.
pub fn epoch() -> &'static str {
    static EPOCH: OnceLock<String> = OnceLock::new();
    EPOCH.get_or_init(|| uuid::Uuid::new_v4().to_string())
}
