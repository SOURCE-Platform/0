//! The line protocol of `claude -p --input-format stream-json --output-format
//! stream-json`: how SOURCE writes a message in, and what each output line means.

use serde_json::{json, Value};

/// One user message, as a single line for the process's stdin.
pub fn encode_user_message(text: &str) -> String {
    json!({ "type": "user", "message": { "role": "user", "content": text } }).to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    Init { session_id: String, model: String },
    AssistantText { text: String },
    ToolUse { name: String },
    ToolResult { is_error: bool },
    /// Claude's own short description of what it's doing ("Running echo hi").
    Progress { detail: String },
    /// Claude's one-line summary after a turn ("ran echo hi; replied DONE").
    TurnSummary { detail: String, needs_action: String },
    /// Plan usage, reported alongside turns.
    Usage { status: String, weekly_utilization: Option<f64> },
    TurnDone(TurnResult),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnResult {
    pub is_error: bool,
    pub text: String,
    pub cost_usd: f64,
    pub duration_ms: i64,
    pub api_error_status: Option<i64>,
    /// Messages still waiting to run after this turn (sent while it was busy).
    pub queued_turn_count: i64,
}

impl TurnResult {
    /// The command-line Claude keeps its own sign-in, separate from the Claude
    /// app's, and reports an expired one as ordinary reply text.
    pub fn is_auth_error(&self) -> bool {
        self.api_error_status == Some(401)
            || self.text.contains("Failed to authenticate")
            || self.text.contains("Not logged in")
            || self.text.contains("Run /login")
            || self.text.contains("run /login")
            || (self.text.contains("401")
                && (self.text.contains("authenticate") || self.text.contains("OAuth")))
    }
}

/// Parse one output line. A line can hold several content blocks, so it can
/// produce several events; bookkeeping lines produce none.
pub fn parse_line(line: &str) -> Vec<StreamEvent> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    // Subagent traffic carries the tool call it belongs to; it isn't this conversation's reply.
    if value.get("parent_tool_use_id").is_some_and(|id| !id.is_null()) {
        return Vec::new();
    }
    match text(&value, "type").as_str() {
        "system" => parse_system(&value).into_iter().collect(),
        "assistant" => blocks(&value)
            .filter_map(|block| match text(block, "type").as_str() {
                "text" if !text(block, "text").trim().is_empty() => {
                    Some(StreamEvent::AssistantText { text: text(block, "text") })
                }
                "tool_use" => Some(StreamEvent::ToolUse { name: text(block, "name") }),
                _ => None,
            })
            .collect(),
        "user" => blocks(&value)
            .filter(|block| text(block, "type") == "tool_result")
            .map(|block| StreamEvent::ToolResult {
                is_error: block.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            })
            .collect(),
        "rate_limit_event" => {
            let info = value.get("rate_limit_info");
            vec![StreamEvent::Usage {
                status: info.map(|i| text(i, "status")).unwrap_or_default(),
                weekly_utilization: info
                    .and_then(|i| i.pointer("/unifiedWindows/seven_day/utilization"))
                    .and_then(Value::as_f64),
            }]
        }
        "result" => vec![StreamEvent::TurnDone(TurnResult {
            is_error: value.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            text: text(&value, "result"),
            cost_usd: value.get("total_cost_usd").and_then(Value::as_f64).unwrap_or(0.0),
            duration_ms: value.get("duration_ms").and_then(Value::as_i64).unwrap_or(0),
            api_error_status: value.get("api_error_status").and_then(Value::as_i64),
            queued_turn_count: value.get("queued_turn_count").and_then(Value::as_i64).unwrap_or(0),
        })],
        _ => Vec::new(),
    }
}

fn parse_system(value: &Value) -> Option<StreamEvent> {
    match text(value, "subtype").as_str() {
        "init" => Some(StreamEvent::Init {
            session_id: text(value, "session_id"),
            model: text(value, "model"),
        }),
        "task_summary" => {
            let detail = text(value, "detail");
            (!detail.is_empty()).then_some(StreamEvent::Progress { detail })
        }
        "post_turn_summary" => Some(StreamEvent::TurnSummary {
            detail: text(value, "status_detail"),
            needs_action: text(value, "needs_action"),
        }),
        _ => None,
    }
}

fn blocks(value: &Value) -> impl Iterator<Item = &Value> {
    value
        .pointer("/message/content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}
