use serde::{Deserialize, Serialize};

/// The coding-agent apps SOURCE can read sessions from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentApp {
    Codex,
    ClaudeCode,
    Factory,
    OpenCode,
}

impl AgentApp {
    pub fn label(self) -> &'static str {
        match self {
            AgentApp::Codex => "Codex",
            AgentApp::ClaudeCode => "Claude Code",
            AgentApp::Factory => "Factory",
            AgentApp::OpenCode => "OpenCode",
        }
    }
}

/// One conversation in one of those apps, flattened to the fields the hub shows.
///
/// Everything here comes from files the apps already keep on disk, so reading a
/// session never disturbs the app that owns it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    pub id: String,
    pub app: AgentApp,
    pub title: String,
    /// Working directory the session runs in, as the app recorded it.
    pub project_path: String,
    /// Last path component of `project_path`, for grouping in the UI.
    pub project_name: String,
    pub updated_at_ms: i64,
    /// Short excerpt so a row says something without opening the session.
    pub preview: String,
    /// True when a process is running this session right now.
    pub live: bool,
    pub archived: bool,
}

/// Cut a title or preview down to something a row can show.
pub fn shorten(text: &str, max_chars: usize) -> String {
    let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.chars().count() <= max_chars {
        return cleaned;
    }
    let kept: String = cleaned.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// Folder name of a working directory ("/Users/a/Documents/0" -> "0").
pub fn project_name_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Fall back to the first line of the first prompt when an app stored no title.
pub fn title_or_prompt(title: &str, prompt: &str, id: &str) -> String {
    for candidate in [title, prompt] {
        let trimmed = candidate.trim();
        if !trimmed.is_empty() {
            return shorten(trimmed, 70);
        }
    }
    format!("Untitled session {}", &id[..id.len().min(8)])
}
