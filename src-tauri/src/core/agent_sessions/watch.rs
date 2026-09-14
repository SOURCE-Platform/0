//! Tell the hub when agent sessions change, using the operating system's file
//! change notifications (FSEvents on macOS) instead of re-reading on a timer.

use super::paths::AgentRoots;
use notify::{Event, RecursiveMode, Watcher};
use std::path::Path;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

/// Changes arrive in bursts (a reply is several writes), so wait for this much
/// quiet before telling anyone.
const SETTLE: Duration = Duration::from_millis(300);

/// Keeps the watch alive; dropping it stops watching.
pub struct AgentChangeWatch {
    _watcher: notify::RecommendedWatcher,
}

/// Start watching. The receiver's value increases each time something settles.
pub fn watch_agent_changes(roots: &AgentRoots) -> Result<(AgentChangeWatch, watch::Receiver<u64>), String> {
    let (raw_tx, raw_rx) = mpsc::unbounded_channel::<()>();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
        if let Ok(event) = result {
            if event.paths.iter().any(|path| is_relevant(path)) {
                let _ = raw_tx.send(());
            }
        }
    })
    .map_err(|error| format!("Could not watch agent sessions: {error}"))?;

    let targets = [
        (roots.claude.join("sessions"), RecursiveMode::NonRecursive),
        (roots.claude.join("projects"), RecursiveMode::Recursive),
        (roots.codex.clone(), RecursiveMode::NonRecursive),
        (roots.factory.clone(), RecursiveMode::NonRecursive),
        (roots.opencode.clone(), RecursiveMode::NonRecursive),
    ];
    for (path, mode) in targets {
        // An app that isn't installed just has nothing to watch.
        if path.exists() {
            let _ = watcher.watch(&path, mode);
        }
    }

    let (changes_tx, changes_rx) = watch::channel(0u64);
    tokio::spawn(settle(raw_rx, changes_tx));
    Ok((AgentChangeWatch { _watcher: watcher }, changes_rx))
}

/// Only files that describe sessions count; everything else those folders hold
/// (logs, caches, attachments) changes constantly and means nothing here.
pub(crate) fn is_relevant(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let in_folder = |folder: &str| path.parent().and_then(|p| p.file_name()).is_some_and(|n| n == folder);
    (in_folder("sessions") && name.ends_with(".json"))
        || name.ends_with(".jsonl")
        || name.starts_with("state_") && name.contains(".sqlite")
        || name.starts_with("opencode.db")
        || name == "sessions-index.json"
}

async fn settle(mut raw: mpsc::UnboundedReceiver<()>, changes: watch::Sender<u64>) {
    while raw.recv().await.is_some() {
        // Absorb the rest of the burst: each new change restarts the quiet period.
        loop {
            match tokio::time::timeout(SETTLE, raw.recv()).await {
                Ok(Some(())) => continue,
                Ok(None) => return,
                Err(_) => break,
            }
        }
        changes.send_modify(|count| *count += 1);
    }
}
