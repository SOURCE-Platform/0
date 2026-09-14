//! Turn one line of a Claude Code transcript into the few things the hub cares
//! about. A line can hold several content blocks, so one line can produce
//! several records; most lines (attachments, snapshots, queue bookkeeping)
//! produce none.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClaudeRecord {
    UserPrompt { uuid: String, text: String, at_ms: i64 },
    PeerMessage { uuid: String, from_name: String, body: String, at_ms: i64 },
    AssistantText { uuid: String, message_id: String, text: String, at_ms: i64 },
    ToolUse { uuid: String, name: String, at_ms: i64 },
    ToolResult { uuid: String, is_error: bool, at_ms: i64 },
}

/// Prompts that Claude Code writes on the user's behalf for slash commands.
/// They are bookkeeping, not something the user said.
const COMMAND_MARKERS: [&str; 4] = [
    "<command-name>",
    "<command-message>",
    "<local-command-stdout>",
    "<local-command-caveat>",
];

pub(crate) fn parse_line(line: &str) -> Vec<ClaudeRecord> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    // Subagent chatter is written to the same file; it isn't the conversation.
    if value.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return Vec::new();
    }
    let uuid = str_field(&value, "uuid");
    let at_ms = timestamp_ms(&value);
    match str_field(&value, "type").as_str() {
        "user" => parse_user(&value, uuid, at_ms),
        "assistant" => parse_assistant(&value, uuid, at_ms),
        _ => Vec::new(),
    }
}

fn parse_user(value: &Value, uuid: String, at_ms: i64) -> Vec<ClaudeRecord> {
    let origin = value.get("origin");
    if origin.and_then(|o| o.get("kind")).and_then(Value::as_str) == Some("peer") {
        let origin = origin.expect("checked above");
        return vec![ClaudeRecord::PeerMessage {
            uuid,
            from_name: str_field(origin, "name"),
            body: str_field(origin, "body"),
            at_ms,
        }];
    }
    if value.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return Vec::new();
    }
    match value.pointer("/message/content") {
        Some(Value::String(text)) => user_text(uuid, text, at_ms).into_iter().collect(),
        Some(Value::Array(blocks)) => {
            let mut records: Vec<ClaudeRecord> = blocks
                .iter()
                .filter(|block| str_field(block, "type") == "tool_result")
                .map(|block| ClaudeRecord::ToolResult {
                    uuid: uuid.clone(),
                    is_error: block.get("is_error").and_then(Value::as_bool).unwrap_or(false),
                    at_ms,
                })
                .collect();
            let text = blocks
                .iter()
                .filter(|block| str_field(block, "type") == "text")
                .map(|block| str_field(block, "text"))
                .collect::<Vec<_>>()
                .join("\n");
            records.extend(user_text(uuid, &text, at_ms));
            records
        }
        _ => Vec::new(),
    }
}

fn user_text(uuid: String, text: &str, at_ms: i64) -> Option<ClaudeRecord> {
    let trimmed = text.trim();
    if trimmed.is_empty() || COMMAND_MARKERS.iter().any(|marker| trimmed.starts_with(marker)) {
        return None;
    }
    Some(ClaudeRecord::UserPrompt { uuid, text: trimmed.to_string(), at_ms })
}

fn parse_assistant(value: &Value, uuid: String, at_ms: i64) -> Vec<ClaudeRecord> {
    let message_id = value
        .pointer("/message/id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let Some(Value::Array(blocks)) = value.pointer("/message/content") else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter_map(|block| match str_field(block, "type").as_str() {
            "text" => {
                let text = str_field(block, "text");
                (!text.trim().is_empty()).then(|| ClaudeRecord::AssistantText {
                    uuid: uuid.clone(),
                    message_id: message_id.clone(),
                    text,
                    at_ms,
                })
            }
            "tool_use" => Some(ClaudeRecord::ToolUse {
                uuid: uuid.clone(),
                name: str_field(block, "name"),
                at_ms,
            }),
            _ => None,
        })
        .collect()
}

fn str_field(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn timestamp_ms(value: &Value) -> i64 {
    value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
        .map(|time| time.timestamp_millis())
        .unwrap_or(0)
}
