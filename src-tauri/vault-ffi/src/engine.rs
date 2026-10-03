//! The engine behind one handle: the core, its services, the op lane and
//! the auto-lock tick (spec §1.6 on the phone, review SEC-B1 / VER-I4).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
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
}

impl Inner {
    pub fn boot(dir: PathBuf, shared: Arc<Shared>) -> Arc<Inner> {
        let deps = shared.deps();
        let inner = Arc::new(Inner { core: Arc::new(Mutex::new(VaultCore::boot(dir))), shared, deps, lane: Mutex::new(()), stop: AtomicBool::new(false) });
        let tick = Arc::clone(&inner);
        std::thread::spawn(move || tick.tick());
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

    /// The window since the last authorization, once a second, as the
    /// helper's tick does; ends with the handle.
    fn tick(&self) {
        while !self.stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_secs(1));
            if !self.stop.load(Ordering::SeqCst) && lock_core(&self.core).auto_lock_due() {
                self.lock(LockReason::Timeout);
            }
        }
    }

    pub fn run(&self, frame: &Value) -> OpOutcome {
        let op = frame.get("op").and_then(Value::as_str).unwrap_or("");
        if !IOS_OPS.contains(&op) {
            return OpOutcome::err(ErrorCode::UnknownOp);
        }
        // Never act on a window that has already run out.
        if lock_core(&self.core).auto_lock_due() {
            self.lock(LockReason::Timeout);
        }
        if op == "get_state" {
            // Answered at once, like the helper's server layer.
            return OpOutcome { response: lock_core(&self.core).state_answer() };
        }
        let _lane = self.lane.lock().unwrap_or_else(|p| p.into_inner());
        dispatch::dispatch(&self.core, frame, &self.deps)
    }
}
