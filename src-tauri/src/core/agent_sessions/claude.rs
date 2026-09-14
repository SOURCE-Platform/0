use super::claude_history::read_tail;
use super::claude_registry::read_registry;
use super::paths::AgentRoots;
use super::types::{project_name_from_path, shorten, title_or_prompt, AgentApp, AgentSession};
use std::collections::HashMap;

/// Only the tail of a transcript is read: enough for the title, the last reply
/// and the working directory, without loading long conversations.
const TAIL_BYTES: u64 = 128 * 1024;

#[derive(Debug, Default)]
pub(crate) struct Parsed {
    pub title: String,
    pub prompt: String,
    pub preview: String,
    pub cwd: String,
}

/// Sessions Claude Code has running right now, keyed by session id, with the
/// running process's name.
fn live_sessions(roots: &AgentRoots) -> HashMap<String, String> {
    read_registry(roots)
        .into_iter()
        .map(|session| (session.session_id, session.name))
        .collect()
}

/// Read the last chunk of a transcript and pull out what a row needs.
pub(crate) fn parse_tail(text: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let mut lines: Vec<&str> = text.lines().collect();
    if lines.len() > 1 && !text.starts_with('{') {
        lines.remove(0); // partial first line from the seek
    }
    for line in lines.iter().rev() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if parsed.cwd.is_empty() {
            if let Some(cwd) = value.get("cwd").and_then(|v| v.as_str()) {
                parsed.cwd = cwd.to_string();
            }
        }
        match value.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "custom-title" if parsed.title.is_empty() => {
                if let Some(title) = value.get("customTitle").and_then(|v| v.as_str()) {
                    parsed.title = title.to_string();
                }
            }
            "last-prompt" if parsed.prompt.is_empty() => {
                if let Some(prompt) = value.get("lastPrompt").and_then(|v| v.as_str()) {
                    parsed.prompt = prompt.to_string();
                }
            }
            "assistant" if parsed.preview.is_empty() => {
                parsed.preview = message_text(&value);
            }
            "user" if parsed.prompt.is_empty() => {
                parsed.prompt = message_text(&value);
            }
            _ => {}
        }
        if !parsed.title.is_empty() && !parsed.preview.is_empty() && !parsed.cwd.is_empty() {
            break;
        }
    }
    parsed
}

/// Message content is either a plain string or a list of blocks.
fn message_text(value: &serde_json::Value) -> String {
    let content = value.pointer("/message/content");
    match content {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// Claude Code stores one JSONL transcript per session under `projects/`.
pub fn list(roots: &AgentRoots, limit: usize) -> Result<Vec<AgentSession>, String> {
    let projects = roots.claude.join("projects");
    if !projects.exists() {
        return Ok(Vec::new());
    }
    let live = live_sessions(roots);

    let mut files: Vec<(std::path::PathBuf, i64)> = Vec::new();
    for project in std::fs::read_dir(&projects).map_err(|e| e.to_string())?.flatten() {
        let Ok(entries) = std::fs::read_dir(project.path()) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let modified = entry
                .metadata()
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|since| since.as_millis() as i64)
                .unwrap_or(0);
            files.push((path, modified));
        }
    }
    files.sort_by(|a, b| b.1.cmp(&a.1));
    files.truncate(limit);

    Ok(files
        .into_iter()
        .map(|(path, modified)| {
            let id = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let parsed = read_tail(&path, TAIL_BYTES).map(|text| parse_tail(&text)).unwrap_or_default();
            let live_name = live.get(&id);
            // A title the user set wins; otherwise the running session's own
            // name; otherwise the first line of what was asked.
            let named = if parsed.title.is_empty() {
                live_name.map(String::as_str).unwrap_or_default()
            } else {
                parsed.title.as_str()
            };
            AgentSession {
                title: title_or_prompt(named, &parsed.prompt, &id),
                app: AgentApp::ClaudeCode,
                project_name: project_name_from_path(&parsed.cwd),
                project_path: parsed.cwd,
                updated_at_ms: modified,
                preview: shorten(&parsed.preview, 140),
                live: live_name.is_some(),
                archived: false,
                id,
            }
        })
        .collect())
}
