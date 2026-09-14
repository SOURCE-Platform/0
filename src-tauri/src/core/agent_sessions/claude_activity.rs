//! Is a Claude Code conversation in the middle of a turn? Read from the end of
//! its transcript, so it works for sessions SOURCE doesn't run itself.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnActivity {
    /// The last turn finished; nothing is waiting.
    Idle,
    /// Claude is working, running a tool, waiting for approval, or has a message queued.
    Busy,
}

/// Stop reasons that end a turn. `tool_use` (and a missing reason) means Claude
/// is about to act or is still writing.
const TURN_ENDING: [&str; 4] = ["end_turn", "stop_sequence", "max_tokens", "refusal"];

/// Decide from the newest records backwards.
pub fn turn_activity(transcript_tail: &str) -> TurnActivity {
    let mut queued = 0i32;
    for line in transcript_tail.lines().rev() {
        let Ok(value) = serde_json::from_str::<Value>(line) else { continue };
        if value.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        match value.get("type").and_then(Value::as_str).unwrap_or_default() {
            // Seen newest-first: a remove/dequeue cancels an older enqueue.
            "queue-operation" => match value.get("operation").and_then(Value::as_str) {
                Some("enqueue") => queued += 1,
                Some("dequeue") | Some("remove") => queued -= 1,
                _ => {}
            },
            "assistant" => {
                if queued > 0 {
                    return TurnActivity::Busy;
                }
                let stop = value.pointer("/message/stop_reason").and_then(Value::as_str);
                return if stop.is_some_and(|reason| TURN_ENDING.contains(&reason)) {
                    TurnActivity::Idle
                } else {
                    TurnActivity::Busy
                };
            }
            "user" => {
                if value.get("isMeta").and_then(Value::as_bool) == Some(true) {
                    continue;
                }
                // A prompt or tool result with no reply after it: Claude owes an answer.
                return TurnActivity::Busy;
            }
            _ => {}
        }
    }
    if queued > 0 {
        TurnActivity::Busy
    } else {
        TurnActivity::Idle
    }
}
