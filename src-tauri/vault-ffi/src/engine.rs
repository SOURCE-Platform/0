//! The engine behind one handle: the core, its services, the op lane and
//! the auto-lock tick (spec §1.6 on the phone, review SEC-B1 / VER-I4).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::Value;
use vault_engine::errors::ErrorCode;
use vault_engine::vault::{dispatch, lock_core, Deps, LockReason, OpOutcome, VaultCore};

use crate::callbacks::Shared;

/// The §1.5 ops SOURCE Vault may run (F.2b). Everything else answers
/// `UNKNOWN_OP` — the Mac-only serving, enrollment authorization, vault
/// creation, revocation and total-loss recovery ops, and
/// `rotate_recovery_key`, whose phone form (rotation staged before the
/// sheet, §22.10) does not exist yet (review SEC-B2 / VER-B1).
pub const IOS_OPS: &[&str] = &[
    "get_state", "unlock", "begin_recovery_unlock", "list_items", "reveal", "add_item", "update_item", "delete_item",
    "list_history", "list_deleted", "restore_revision", "resolve_conflict", "change_master_password", "list_devices",
    "registry_status", "set_auto_lock_minutes", "backup_prepare", "backup_blob_list", "backup_transition_body",
    "backup_commit_result", "backup_state_offer", "backup_apply", "stream_read", "stream_begin", "stream_write", "stream_end",
    "stream_cancel", "sign_provider_request", "session_close", "quarantine_status", "remote_update_status",
    // F.2b step 5: the phone's side of enrollment (§5, §22.10).
    "join_begin", "join_hello", "join_bundle_begin", "join_complete", "join_finish", "join_abort",
    // F.2c: the phone's peer exchange (§22.8).
    "peer_sync_begin", "peer_sync_step",
];

/// The helper's frame cap (spec §1.4, `ipc/framing.rs`), kept here too.
pub const MAX_REQUEST: usize = 64 * 1024;

pub struct Inner {
    pub core: Arc<Mutex<VaultCore>>,
    pub shared: Arc<Shared>,
    pub deps: Deps,
    /// One op at a time, as the helper's executor runs them. `get_state`,
    /// `lock` and the tick never take it.
    pub lane: Mutex<()>,
    pub stop: AtomicBool,
    /// The auto-lock tick; joined by `close`, so no engine thread calls
    /// Swift after `ov0_engine_close` returns (review VER-I10 / SEC-I1).
    pub tick: Mutex<Option<JoinHandle<()>>>,
}

impl Inner {
    pub fn boot(dir: PathBuf, shared: Arc<Shared>) -> Arc<Inner> {
        let deps = shared.deps();
        let inner = Arc::new(Inner {
            core: Arc::new(Mutex::new(VaultCore::boot_phone(dir))),
            shared,
            deps,
            lane: Mutex::new(()),
            stop: AtomicBool::new(false),
            tick: Mutex::new(None),
        });
        // The tick holds a weak reference: it never keeps the engine alive,
        // and a panic in it aborts like any entry point (review VER-I11).
        let weak = Arc::downgrade(&inner);
        let handle = std::thread::spawn(move || crate::guarded(|| tick(weak)));
        *inner.tick.lock().unwrap_or_else(|p| p.into_inner()) = Some(handle);
        inner
    }

    /// Lock now, and hand Swift the `locked` event — its cue to dismiss
    /// any secure entry or sheet still on screen.
    pub fn lock(&self, reason: LockReason) {
        let events = lock_core(&self.core).lock(reason);
        for ev in events {
            self.deps.events.emit(ev);
        }
        self.shared.flush();
    }

    /// Stop the tick and wait for it (it is parked, so it wakes at once).
    pub fn end_tick(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.tick.lock().unwrap_or_else(|p| p.into_inner()).take() {
            h.thread().unpark();
            let _ = h.join();
        }
    }

    fn lock_if_due(&self) {
        if lock_core(&self.core).auto_lock_due() {
            self.lock(LockReason::Timeout);
        }
    }

    pub fn run(&self, frame: &Value) -> OpOutcome {
        let op = frame.get("op").and_then(Value::as_str).unwrap_or("");
        if !IOS_OPS.contains(&op) {
            return OpOutcome::err(ErrorCode::UnknownOp);
        }
        // Never act on a window that has already run out.
        self.lock_if_due();
        if op == "get_state" {
            // Answered at once, like the helper's server layer.
            return OpOutcome { response: lock_core(&self.core).state_answer() };
        }
        let _lane = self.lane.lock().unwrap_or_else(|p| p.into_inner());
        self.lock_if_due(); // again: the wait for the lane may have been long
        dispatch::dispatch(&self.core, frame, &self.deps)
    }
}

/// The §1.6 window since the last authorization, once a second, as the
/// helper's tick does; ends when the handle stops it or is gone.
fn tick(engine: std::sync::Weak<Inner>) {
    loop {
        std::thread::park_timeout(Duration::from_secs(1));
        let Some(e) = engine.upgrade() else { return };
        if e.stop.load(Ordering::SeqCst) {
            return;
        }
        e.lock_if_due();
    }
}
