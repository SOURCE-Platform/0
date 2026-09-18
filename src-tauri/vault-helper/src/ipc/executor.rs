//! The single ops executor (spec §1.4/§13.3). All state-mutating ops run
//! here, serially, so vault-state transitions never interleave and at
//! most one secure panel / LA sheet is outstanding at a time.
//!
//! `lock`, `get_state`, and the auto-lock tick deliberately run OUTSIDE
//! this queue: they only zeroize, and the §13.3 design requires them to
//! land under an in-flight op's feet (the op then fails closed at its
//! re-lock checkpoint). See `vault/mod.rs` for the lock discipline.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde_json::Value;

use crate::vault::{self, Deps, VaultCore};

/// One op frame plus the channel its response travels back on.
pub struct Inbound {
    pub frame: Value,
    pub reply: SyncSender<Value>,
}

/// Spawn the executor thread. Runs until every `Inbound` sender is
/// dropped (server shutdown). `panel_cancel` is the §13.3 preemption
/// flag: `lock`/auto-lock set it, the panel runner aborts a visible
/// panel, and the executor clears it before the next op begins — the
/// queue's seriality makes that consume-once protocol race-free.
pub fn spawn(
    core: Arc<Mutex<VaultCore>>,
    deps: Deps,
    panel_cancel: Arc<AtomicBool>,
    rx: Receiver<Inbound>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        while let Ok(inbound) = rx.recv() {
            panel_cancel.store(false, Ordering::SeqCst);
            let outcome = vault::dispatch(&core, &inbound.frame, &deps);
            // A vanished client must not wedge the queue; drop the answer.
            let _ = inbound.reply.send(outcome.response);
        }
    })
}
