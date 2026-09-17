use super::brief::brief_reply;
use super::claude_cli::resolve_claude_binary;
use super::claude_driver::{ClaudeDriver, DriverConfig, RELEASE_AFTER_QUIET};
use super::driver_events::DriverEvent;
use super::handoff::{take_over, HandoffError};
use crate::core::agent_sessions::{claude_conversation_context, AgentRoots};
use crate::core::config::Config;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex};

/// Said once if Claude has been quiet this long mid-turn, so a long task
/// doesn't look like a dropped one.
const STILL_WORKING_AFTER: Duration = Duration::from_secs(90);

/// Something that happened in a conversation SOURCE is driving.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeEvent {
    pub session_id: String,
    pub event: DriverEvent,
    /// Present when a turn finished: the reply, short enough to say out loud.
    pub brief: Option<String>,
    pub still_working: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SendError {
    Handoff(HandoffError),
    /// Neither the Claude app's copy of `claude` nor an installed one was found.
    NoClaudeProgram,
    /// The transcript doesn't say which folder the conversation runs in.
    NoConversation,
    Driver { message: String },
}

/// Sends prompts into Claude Code conversations: takes a conversation over
/// from the Claude app when needed, drives it, and broadcasts what happens.
pub struct AgentBridge {
    roots: AgentRoots,
    binary: Option<PathBuf>,
    /// The app's settings, read when a conversation's process starts. Tests go without.
    config: Option<Arc<std::sync::Mutex<Config>>>,
    release_after_quiet: Duration,
    drivers: Mutex<HashMap<String, Arc<ClaudeDriver>>>,
    events: broadcast::Sender<BridgeEvent>,
}

impl AgentBridge {
    pub fn new(roots: AgentRoots, config: Arc<std::sync::Mutex<Config>>) -> Arc<Self> {
        Self::build(roots, resolve_claude_binary(), RELEASE_AFTER_QUIET, Some(config))
    }

    pub fn with_binary(roots: AgentRoots, binary: Option<PathBuf>, release_after_quiet: Duration) -> Arc<Self> {
        Self::build(roots, binary, release_after_quiet, None)
    }

    fn build(
        roots: AgentRoots,
        binary: Option<PathBuf>,
        release_after_quiet: Duration,
        config: Option<Arc<std::sync::Mutex<Config>>>,
    ) -> Arc<Self> {
        let (events, _) = broadcast::channel(512);
        Arc::new(Self { roots, binary, config, release_after_quiet, drivers: Mutex::new(HashMap::new()), events })
    }

    fn keeps_bypass(&self) -> bool {
        let Some(config) = &self.config else { return false };
        config.lock().map(|config| config.agent_prompts_keep_bypass).unwrap_or(false)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BridgeEvent> {
        self.events.subscribe()
    }

    /// Conversations SOURCE currently holds (its process is running).
    pub async fn held_sessions(&self) -> Vec<String> {
        self.drivers
            .lock()
            .await
            .iter()
            .filter(|(_, driver)| driver.pid().is_some())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Send one prompt into a conversation.
    pub async fn send_prompt(self: &Arc<Self>, session_id: &str, text: &str) -> Result<DriverEvent, SendError> {
        let existing = self.drivers.lock().await.get(session_id).cloned();
        let own_pids: Vec<u32> = existing.as_ref().and_then(|d| d.pid()).into_iter().collect();
        take_over(&self.roots, session_id, &own_pids).await.map_err(SendError::Handoff)?;

        let driver = match existing {
            Some(driver) => driver,
            None => self.start_driver(session_id).await?,
        };
        driver
            .send(text)
            .await
            .map_err(|error| SendError::Driver { message: format!("{error:?}") })
    }

    /// Hand a conversation back to the Claude app now.
    pub async fn release(&self, session_id: &str) {
        let driver = self.drivers.lock().await.get(session_id).cloned();
        if let Some(driver) = driver {
            driver.release().await;
        }
    }

    async fn start_driver(self: &Arc<Self>, session_id: &str) -> Result<Arc<ClaudeDriver>, SendError> {
        let binary = self.binary.clone().ok_or(SendError::NoClaudeProgram)?;
        let context = claude_conversation_context(&self.roots, session_id).ok_or(SendError::NoConversation)?;
        let driver = ClaudeDriver::new(DriverConfig {
            binary,
            session_id: session_id.to_string(),
            cwd: PathBuf::from(context.cwd),
            permission_mode: context.permission_mode,
            keep_bypass: self.keeps_bypass(),
            release_after_quiet: self.release_after_quiet,
        });
        tokio::spawn(forward(driver.subscribe(), self.events.clone(), driver.clone()));
        self.drivers.lock().await.insert(session_id.to_string(), driver.clone());
        Ok(driver)
    }
}

/// Relay one driver's events, adding the brief reply and a single "still
/// working" note when a turn goes quiet for a long time.
async fn forward(
    mut events: broadcast::Receiver<DriverEvent>,
    out: broadcast::Sender<BridgeEvent>,
    driver: Arc<ClaudeDriver>,
) {
    let session_id = driver.session_id().to_string();
    let mut announced_still_working = false;
    loop {
        let wait = if driver.is_busy() && !announced_still_working {
            STILL_WORKING_AFTER
        } else {
            Duration::MAX
        };
        let event = tokio::select! {
            received = events.recv() => match received {
                Ok(event) => event,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return,
            },
            _ = sleep_or_forever(wait) => {
                announced_still_working = true;
                let _ = out.send(BridgeEvent {
                    session_id: session_id.clone(),
                    event: DriverEvent::Progress { detail: "Still working".to_string() },
                    brief: None,
                    still_working: true,
                });
                continue;
            }
        };
        let brief = match &event {
            DriverEvent::TurnDone { text, summary, .. } => Some(brief_reply(text, summary.as_deref())),
            _ => None,
        };
        if matches!(event, DriverEvent::Working | DriverEvent::TurnDone { .. }) {
            announced_still_working = false;
        }
        let _ = out.send(BridgeEvent { session_id: session_id.clone(), event, brief, still_working: false });
    }
}

async fn sleep_or_forever(duration: Duration) {
    if duration == Duration::MAX {
        std::future::pending::<()>().await;
    } else {
        tokio::time::sleep(duration).await;
    }
}
