use super::paths::AgentRoots;
use serde::Serialize;

/// A Claude Code process that is running right now, from the registry file it
/// writes at `~/.claude/sessions/<pid>.json`.
///
/// The Claude desktop app and any `claude` process SOURCE starts both write one,
/// so a conversation can have more than one entry. Names are derived and change
/// over a session's life; the session id doesn't, so always look up by id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveClaudeSession {
    pub pid: i32,
    pub session_id: String,
    pub name: String,
    pub cwd: String,
    pub entrypoint: String,
}

/// Parse one registry file. `None` for anything that isn't a usable entry.
pub(crate) fn parse_registry_entry(raw: &str) -> Option<LiveClaudeSession> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let text = |key: &str| value.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let pid = i32::try_from(value.get("pid")?.as_i64()?).ok()?;
    let session_id = text("sessionId");
    if session_id.is_empty() || pid <= 0 {
        return None;
    }
    Some(LiveClaudeSession {
        pid,
        session_id,
        name: text("name"),
        cwd: text("cwd"),
        entrypoint: text("entrypoint"),
    })
}

/// Every registered Claude process whose pid is still alive.
///
/// Registry files outlive a crashed process, so the pid is checked rather than
/// trusting the file's existence.
pub fn read_registry(roots: &AgentRoots) -> Vec<LiveClaudeSession> {
    let Ok(entries) = std::fs::read_dir(roots.claude.join("sessions")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().extension().and_then(|e| e.to_str()) == Some("json"))
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .filter_map(|raw| parse_registry_entry(&raw))
        .filter(|session| pid_alive(session.pid))
        .collect()
}

/// All running processes for one conversation (usually zero or one).
pub fn processes_for(roots: &AgentRoots, session_id: &str) -> Vec<LiveClaudeSession> {
    read_registry(roots)
        .into_iter()
        .filter(|session| session.session_id == session_id)
        .collect()
}

/// `kill(pid, 0)` asks whether the process exists without signalling it.
/// `EPERM` still means it exists; it just belongs to someone else.
pub(crate) fn pid_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 performs only the existence and permission check.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}
