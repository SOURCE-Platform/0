//! Read-only index of coding-agent sessions across Codex, Claude Code,
//! Factory and OpenCode.
//!
//! Every app already keeps its own session records on disk. This module reads
//! those records and nothing else: no agent process is started, and nothing is
//! written back, so opening the hub can never disturb a running session.

mod claude;
mod codex;
mod factory;
mod opencode;
mod paths;
mod types;

#[cfg(test)]
mod tests;

pub use paths::AgentRoots;
pub use types::{AgentApp, AgentSession};

/// Sessions from every app, newest first.
///
/// `limit` applies per app before merging, so one busy app cannot crowd the
/// others out of the list.
pub async fn list_agent_sessions(limit: usize) -> Vec<AgentSession> {
    list_from(&AgentRoots::from_env(), limit).await
}

pub async fn list_from(roots: &AgentRoots, limit: usize) -> Vec<AgentSession> {
    let mut sessions = Vec::new();
    collect("Codex", codex::list(roots, limit).await, &mut sessions);
    collect("OpenCode", opencode::list(roots, limit).await, &mut sessions);
    collect("Factory", factory::list(roots, limit), &mut sessions);
    collect("Claude Code", claude::list(roots, limit), &mut sessions);

    // Live sessions first, then most recent. A missing timestamp sorts last
    // instead of jumping to the top.
    sessions.sort_by(|a, b| b.live.cmp(&a.live).then(b.updated_at_ms.cmp(&a.updated_at_ms)));
    sessions
}

/// One unreadable app must not empty the whole hub.
fn collect(app: &str, result: Result<Vec<AgentSession>, String>, into: &mut Vec<AgentSession>) {
    match result {
        Ok(sessions) => into.extend(sessions),
        Err(error) => eprintln!("[agent_sessions] {app} sessions unavailable: {error}"),
    }
}
