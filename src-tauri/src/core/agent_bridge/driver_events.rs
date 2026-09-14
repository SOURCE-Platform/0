use super::stream_protocol::{StreamEvent, TurnResult};
use serde::Serialize;

/// What a conversation SOURCE is driving reports, in the order it happens.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum DriverEvent {
    /// A message was sent and Claude started working on it.
    Working,
    /// A message was sent while Claude was busy; it joins the current work.
    AddedToCurrentWork,
    Progress { detail: String },
    ToolUse { name: String },
    AssistantText { text: String },
    /// A turn finished. `summary` is Claude's own one-line recap, when it wrote one.
    TurnDone {
        text: String,
        summary: Option<String>,
        is_error: bool,
        cost_usd: f64,
        duration_ms: i64,
    },
    Usage { status: String, weekly_utilization: Option<f64> },
    /// The command-line Claude's sign-in has expired; `claude auth login` fixes it.
    AuthExpired,
    /// SOURCE's process for this conversation ended; the conversation is free again.
    Released,
}

/// Tracks whether Claude is mid-turn, from what SOURCE sent and what came back.
#[derive(Debug, Default)]
pub(crate) struct TurnState {
    busy: bool,
    summary: Option<String>,
}

impl TurnState {
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Record a sent message. Returns the event to announce it.
    pub fn on_sent(&mut self) -> DriverEvent {
        let joined = self.busy;
        self.busy = true;
        if joined {
            DriverEvent::AddedToCurrentWork
        } else {
            DriverEvent::Working
        }
    }

    /// Translate one stream event, updating the busy state.
    pub fn on_stream(&mut self, event: StreamEvent) -> Option<DriverEvent> {
        match event {
            StreamEvent::Init { .. } | StreamEvent::ToolResult { .. } => None,
            StreamEvent::AssistantText { text } => Some(DriverEvent::AssistantText { text }),
            StreamEvent::ToolUse { name } => Some(DriverEvent::ToolUse { name }),
            StreamEvent::Progress { detail } => Some(DriverEvent::Progress { detail }),
            StreamEvent::Usage { status, weekly_utilization } => {
                Some(DriverEvent::Usage { status, weekly_utilization })
            }
            // The recap arrives just before the result it describes.
            StreamEvent::TurnSummary { detail, .. } => {
                self.summary = (!detail.trim().is_empty()).then_some(detail);
                None
            }
            StreamEvent::TurnDone(result) => Some(self.finish(result)),
        }
    }

    fn finish(&mut self, result: TurnResult) -> DriverEvent {
        let summary = self.summary.take();
        // A message sent mid-turn runs as a follow-up; Claude is still busy with it.
        if result.queued_turn_count == 0 {
            self.busy = false;
        }
        if result.is_auth_error() {
            self.busy = false;
            return DriverEvent::AuthExpired;
        }
        DriverEvent::TurnDone {
            text: result.text,
            summary,
            is_error: result.is_error,
            cost_usd: result.cost_usd,
            duration_ms: result.duration_ms,
        }
    }

    /// The process ended: whatever was running is over.
    pub fn on_exit(&mut self) {
        self.busy = false;
        self.summary = None;
    }
}
