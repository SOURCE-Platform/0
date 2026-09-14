//! Taking a Claude Code conversation over from the Claude app.
//!
//! The Claude app keeps its own copy of an open conversation in its process and
//! doesn't see turns added from outside. So before SOURCE continues a
//! conversation, that process must be gone. The app starts a fresh one the next
//! time the conversation is used, and that one reads everything SOURCE added.

use crate::core::agent_sessions::{
    claude_processes_for, claude_transcript_path, turn_activity, AgentRoots, LiveClaudeSession,
    TurnActivity,
};
use std::time::Duration;

/// Only the Claude desktop app restarts a conversation's process on its own.
/// A `claude` running in someone's terminal would simply be killed, so it's left alone.
const APP_ENTRYPOINT: &str = "claude-desktop";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// No other process has the conversation.
    Free,
    /// The Claude app has it open and idle: stop these processes first.
    StopAppProcesses(Vec<i32>),
    Refuse(HandoffError),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum HandoffError {
    /// Claude is mid-task in the Claude app; sending now would fork the conversation.
    BusyInApp,
    /// The conversation is open in a terminal `claude`, which wouldn't restart.
    RunningElsewhere { entrypoint: String },
    /// The app's process didn't exit when asked.
    StopFailed { pid: i32 },
    NoTranscript,
}

/// Pure decision, so every case can be tested without real processes.
/// `own_pids` are processes SOURCE started itself; they never block a send.
pub fn decide(processes: &[LiveClaudeSession], own_pids: &[u32], activity: TurnActivity) -> Decision {
    let others: Vec<&LiveClaudeSession> = processes
        .iter()
        .filter(|process| !own_pids.contains(&(process.pid as u32)))
        .collect();
    if others.is_empty() {
        return Decision::Free;
    }
    if let Some(elsewhere) = others.iter().find(|p| p.entrypoint != APP_ENTRYPOINT) {
        return Decision::Refuse(HandoffError::RunningElsewhere { entrypoint: elsewhere.entrypoint.clone() });
    }
    if activity == TurnActivity::Busy {
        return Decision::Refuse(HandoffError::BusyInApp);
    }
    Decision::StopAppProcesses(others.iter().map(|p| p.pid).collect())
}

/// Make the conversation SOURCE's to continue, stopping the Claude app's idle
/// process for it if there is one.
pub async fn take_over(roots: &AgentRoots, session_id: &str, own_pids: &[u32]) -> Result<Decision, HandoffError> {
    let path = claude_transcript_path(roots, session_id).ok_or(HandoffError::NoTranscript)?;
    let tail = crate::core::agent_sessions::claude_read_tail(&path, 256 * 1024).unwrap_or_default();
    let decision = decide(&claude_processes_for(roots, session_id), own_pids, turn_activity(&tail));
    match &decision {
        Decision::Free => {}
        Decision::Refuse(error) => return Err(error.clone()),
        Decision::StopAppProcesses(pids) => {
            for &pid in pids {
                stop_app_process(roots, session_id, pid).await?;
            }
        }
    }
    Ok(decision)
}

/// Stop one process, after re-checking right before signalling that the pid
/// still belongs to this conversation (pids get reused).
async fn stop_app_process(roots: &AgentRoots, session_id: &str, pid: i32) -> Result<(), HandoffError> {
    let still_ours = claude_processes_for(roots, session_id)
        .iter()
        .any(|process| process.pid == pid && process.entrypoint == APP_ENTRYPOINT);
    if !still_ours {
        return Ok(()); // already gone
    }
    let exited = tokio::task::spawn_blocking(move || terminate_and_wait(pid, Duration::from_secs(5)))
        .await
        .unwrap_or(false);
    if exited {
        Ok(())
    } else {
        Err(HandoffError::StopFailed { pid })
    }
}

/// Ask a process to stop and wait for it to exit, using a kqueue exit
/// notification: the kernel wakes this up when it happens, so there's no
/// checking in a loop. The watch is registered before the signal is sent, so a
/// process that exits instantly can't slip past it.
pub(crate) fn terminate_and_wait(pid: i32, timeout: Duration) -> bool {
    // SAFETY: a private kqueue watching one pid for NOTE_EXIT, closed before
    // returning; SIGTERM goes to a pid the caller confirmed.
    unsafe {
        let queue = libc::kqueue();
        if queue < 0 {
            return false;
        }
        let mut change: libc::kevent = std::mem::zeroed();
        change.ident = pid as usize;
        change.filter = libc::EVFILT_PROC;
        change.flags = libc::EV_ADD | libc::EV_ONESHOT;
        change.fflags = libc::NOTE_EXIT;
        if libc::kevent(queue, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) < 0 {
            libc::close(queue);
            // The kernel refuses to watch a process that has already exited.
            return true;
        }
        libc::kill(pid, libc::SIGTERM);
        let deadline = libc::timespec {
            tv_sec: timeout.as_secs() as libc::time_t,
            tv_nsec: timeout.subsec_nanos() as libc::c_long,
        };
        let mut event: libc::kevent = std::mem::zeroed();
        let fired = libc::kevent(queue, std::ptr::null(), 0, &mut event, 1, &deadline);
        libc::close(queue);
        fired > 0
    }
}
