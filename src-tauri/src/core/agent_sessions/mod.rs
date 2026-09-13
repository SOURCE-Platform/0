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

/// What the hub found, plus anything it could not read.
///
/// Problems travel with the result instead of only reaching stderr, because a
/// packaged app has nowhere to print: an app missing from the list would
/// otherwise look like an app with no sessions.
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionsSnapshot {
    pub sessions: Vec<AgentSession>,
    pub problems: Vec<AgentProblem>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProblem {
    pub app: AgentApp,
    pub message: String,
}

/// Sessions from every app, newest first.
///
/// `limit` applies per app before merging, so one busy app cannot crowd the
/// others out of the list.
pub async fn list_agent_sessions(limit: usize) -> AgentSessionsSnapshot {
    list_from(&AgentRoots::from_env(), limit).await
}

pub async fn list_from(roots: &AgentRoots, limit: usize) -> AgentSessionsSnapshot {
    let mut snapshot = AgentSessionsSnapshot::default();
    collect(AgentApp::Codex, codex::list(roots, limit).await, &mut snapshot);
    collect(AgentApp::OpenCode, opencode::list(roots, limit).await, &mut snapshot);
    collect(AgentApp::Factory, factory::list(roots, limit), &mut snapshot);
    collect(AgentApp::ClaudeCode, claude::list(roots, limit), &mut snapshot);

    // Live sessions first, then most recent. A missing timestamp sorts last
    // instead of jumping to the top.
    snapshot
        .sessions
        .sort_by(|a, b| b.live.cmp(&a.live).then(b.updated_at_ms.cmp(&a.updated_at_ms)));
    snapshot
}

/// One unreadable app must not empty the whole hub.
fn collect(
    app: AgentApp,
    result: Result<Vec<AgentSession>, String>,
    into: &mut AgentSessionsSnapshot,
) {
    match result {
        Ok(sessions) => into.sessions.extend(sessions),
        Err(error) => {
            eprintln!("[agent_sessions] {} sessions unavailable: {error}", app.label());
            into.problems.push(AgentProblem { app, message: error });
        }
    }
}
