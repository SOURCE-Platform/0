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
    /// Push-to-talk pressed for a conversation. The binary messages that follow
    /// are its audio: 16 kHz mono Int16 little-endian.
    TalkStart { talk_id: String, session_id: String },
    /// Released: transcribe what was said.
    TalkEnd { talk_id: String },
    /// Slid away while talking: drop the audio.
    TalkCancel { talk_id: String },
    /// During the confirm window: send the transcript now, or don't send it.
    SendNow { talk_id: String },
    CancelSend { talk_id: String },
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
    /// "Let the phone send prompts" was switched on or off on the Mac.
    CanSend { can_send: bool },
    /// Where a push-to-talk prompt has got to.
    Talk { talk_id: String, session_id: String, state: TalkState },
    /// Liveness check; the phone answers `pong`.
    Ping,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum TalkState {
    /// Waiting for speech recognition. `delayed` while Right Option dictation
    /// on the Mac is using the speech engine.
    Transcribing { delayed: bool },
    /// What was heard. It's sent when the window ends unless cancelled.
    Confirm { text: String, send_in_ms: u64 },
    /// Going into the conversation; a `send_result` whose `requestId` is the
    /// talk id follows.
    Sending { text: String },
    NoSpeech,
    Cancelled,
    Failed { message: String },
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
