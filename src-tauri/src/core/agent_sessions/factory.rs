use super::paths::AgentRoots;
use super::types::{project_name_from_path, title_or_prompt, AgentApp, AgentSession};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct FactoryIndex {
    #[serde(default)]
    entries: Vec<FactoryEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FactoryEntry {
    session_id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    cwd: String,
    /// Milliseconds since the epoch, written as a float.
    #[serde(default)]
    mtime: f64,
    #[serde(default)]
    messages_count: i64,
    #[serde(default)]
    archived_at: Option<String>,
}

/// Factory keeps a ready-made index of its sessions, so no directory walk.
pub fn list(roots: &AgentRoots, limit: usize) -> Result<Vec<AgentSession>, String> {
    let index_path = roots.factory.join("sessions-index.json");
    if !index_path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&index_path).map_err(|e| e.to_string())?;
    let index: FactoryIndex = serde_json::from_str(&raw).map_err(|e| e.to_string())?;

    let mut sessions: Vec<AgentSession> = index
        .entries
        .into_iter()
        .filter(|entry| entry.messages_count > 0)
        .map(|entry| AgentSession {
            title: title_or_prompt(&entry.title, "", &entry.session_id),
            id: entry.session_id,
            app: AgentApp::Factory,
            project_name: project_name_from_path(&entry.cwd),
            project_path: entry.cwd,
            updated_at_ms: entry.mtime as i64,
            preview: String::new(),
            live: false,
            archived: entry.archived_at.is_some(),
        })
        .collect();

    sessions.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    sessions.truncate(limit);
    Ok(sessions)
}
