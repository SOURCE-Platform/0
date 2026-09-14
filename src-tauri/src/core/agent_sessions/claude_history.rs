use super::claude_records::{parse_line, ClaudeRecord};
use super::message_types::{AgentMessage, MessageRole};
use super::paths::AgentRoots;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Enough of the end of a transcript for dozens of recent messages, without
/// loading conversations that run to many megabytes.
const HISTORY_TAIL_BYTES: u64 = 512 * 1024;

/// Where a conversation's transcript lives: `projects/<folder>/<session_id>.jsonl`.
/// The folder is derived from the working directory, so search rather than guess.
pub fn transcript_path(roots: &AgentRoots, session_id: &str) -> Option<PathBuf> {
    if session_id.is_empty() || session_id.contains('/') {
        return None;
    }
    let file_name = format!("{session_id}.jsonl");
    std::fs::read_dir(roots.claude.join("projects"))
        .ok()?
        .flatten()
        .map(|project| project.path().join(&file_name))
        .find(|candidate| candidate.is_file())
}

/// The last `limit` readable messages of a conversation, oldest first.
pub fn recent_messages(
    roots: &AgentRoots,
    session_id: &str,
    limit: usize,
) -> Result<Vec<AgentMessage>, String> {
    let path = transcript_path(roots, session_id)
        .ok_or_else(|| format!("No Claude Code transcript found for session {session_id}."))?;
    let text = read_tail(&path, HISTORY_TAIL_BYTES)
        .ok_or_else(|| format!("Could not read {}.", path.display()))?;
    Ok(messages_from_text(&text, limit))
}

/// Where a conversation runs and the permission mode it last ran in, so a
/// prompt SOURCE sends is allowed exactly what one typed in the app would be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationContext {
    pub cwd: String,
    pub permission_mode: Option<String>,
}

pub fn conversation_context(roots: &AgentRoots, session_id: &str) -> Option<ConversationContext> {
    let path = transcript_path(roots, session_id)?;
    context_from_text(&read_tail(&path, HISTORY_TAIL_BYTES)?)
}

/// The newest record wins: both the folder and the mode can change mid-conversation.
pub(crate) fn context_from_text(text: &str) -> Option<ConversationContext> {
    let mut cwd = None;
    let mut permission_mode = None;
    for line in text.lines().rev() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if cwd.is_none() {
            cwd = value.get("cwd").and_then(|v| v.as_str()).map(str::to_string);
        }
        if permission_mode.is_none() {
            permission_mode = value.get("permissionMode").and_then(|v| v.as_str()).map(str::to_string);
        }
        if cwd.is_some() && permission_mode.is_some() {
            break;
        }
    }
    Some(ConversationContext { cwd: cwd?, permission_mode })
}

/// Read the last `bytes` of a file. The first line may be cut in half; callers
/// drop it (a partial line never parses as JSON).
pub(crate) fn read_tail(path: &Path, bytes: u64) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len > bytes {
        file.seek(SeekFrom::Start(len - bytes)).ok()?;
    }
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).ok()?;
    Some(String::from_utf8_lossy(&buffer).to_string())
}

/// Pure conversion from transcript text to messages, so it can be tested on fixtures.
pub(crate) fn messages_from_text(text: &str, limit: usize) -> Vec<AgentMessage> {
    let mut messages: Vec<AgentMessage> = Vec::new();
    // Blocks of one assistant reply are written as separate lines sharing an id.
    let mut open_reply: Option<String> = None;

    for record in text.lines().flat_map(parse_line) {
        match record {
            ClaudeRecord::AssistantText { uuid, message_id, text, at_ms } => {
                let continues = !message_id.is_empty() && open_reply.as_deref() == Some(&message_id);
                match messages.last_mut() {
                    Some(last) if continues && last.role == MessageRole::Assistant => {
                        last.text.push_str("\n\n");
                        last.text.push_str(&text);
                    }
                    _ => messages.push(AgentMessage { id: uuid, role: MessageRole::Assistant, text, at_ms }),
                }
                open_reply = Some(message_id);
                continue;
            }
            ClaudeRecord::UserPrompt { uuid, text, at_ms } => {
                messages.push(AgentMessage { id: uuid, role: MessageRole::User, text, at_ms });
            }
            ClaudeRecord::PeerMessage { uuid, from_name, body, at_ms } => {
                let text = if from_name.is_empty() { body } else { format!("From {from_name}: {body}") };
                messages.push(AgentMessage { id: uuid, role: MessageRole::Peer, text, at_ms });
            }
            ClaudeRecord::ToolUse { uuid, name, at_ms } => {
                messages.push(AgentMessage { id: uuid, role: MessageRole::Tool, text: name, at_ms });
            }
            // Tool output stays out of the conversation view, and doesn't break
            // up the reply around it.
            ClaudeRecord::ToolResult { .. } => continue,
        }
        open_reply = None;
    }

    let skip = messages.len().saturating_sub(limit);
    messages.into_iter().skip(skip).collect()
}
