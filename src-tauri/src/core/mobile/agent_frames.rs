//! Messages on `/v1/agent`, the phone's two-way connection for agent sessions.
//!
//! Every frame is one JSON text message with a snake_case `type`; every field
//! name is camelCase. The phone keeps a mirror
//! of these types (`Net/AgentFrames.swift` in the phone repo); the golden file
//! `fixtures/agent_frames.json` pins the exact shapes both sides must agree on.

use crate::core::agent_bridge::{DriverEvent, SendError};
use crate::core::agent_sessions::{AgentApp, AgentMessage, AgentSession};
use serde::{Deserialize, Serialize};

/// From the phone.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ClientFrame {
    /// First frame after connecting; the Mac answers with a snapshot.
    Hello,
    /// Start following one conversation's messages.
    OpenSession { app: AgentApp, session_id: String },
    /// Stop following the open conversation.
    CloseSession,
    /// A typed prompt. `request_id` comes back on the matching `send_result`.
    SendText { request_id: String, session_id: String, text: String },
    /// Reply to a `ping`.
    Pong,
}

/// To the phone. `seq` counts up per connection so the phone can drop
/// anything it has already applied.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ServerFrame {
    pub seq: u64,
    #[serde(flatten)]
    pub body: ServerBody,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ServerBody {
    /// Everything the phone needs after (re)connecting. `epoch` changes when
    /// the Mac app restarts, telling the phone to forget what it had.
    Snapshot {
        epoch: String,
        sessions: Vec<AgentSession>,
        held: Vec<String>,
        can_send: bool,
    },
    /// The session list changed.
    Sessions { sessions: Vec<AgentSession>, held: Vec<String> },
    /// The open conversation's recent messages, oldest first (replaces the previous list).
    Messages { session_id: String, messages: Vec<AgentMessage> },
    /// Something happened in a conversation SOURCE is driving.
    Turn {
        session_id: String,
        event: DriverEvent,
        brief: Option<String>,
        still_working: bool,
    },
    /// Outcome of a `send_text`.
    SendResult { request_id: String, ok: bool, error: Option<SendError> },
    Error { message: String },
    /// Liveness check; the phone answers `pong`.
    Ping,
}

impl ServerFrame {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"seq":0,"type":"error","message":"encode failed"}"#.into())
    }
}

/// Parse a phone frame; `None` for anything malformed or unknown.
pub fn parse_client_frame(text: &str) -> Option<ClientFrame> {
    serde_json::from_str(text).ok()
}
