//! "Remove this vault" on the iPhone (spec v0.5 §22.9, plan F.2d step 6):
//! the only way a phone's vault is ever deleted. Nothing else deletes it —
//! not a removal by the Mac, not a lost backup — because it may hold the
//! only copy of some items.
//!
//! - `remove_vault_preview` → what would be lost: the revisions only this
//!   phone holds (no provider source, not refused), whether the backup has
//!   corroborated the removal (`access_lost`), and whether the Mac removed
//!   it. With the vault locked the count cannot be read (`unknown`).
//! - `remove_vault {confirm: true}` → presence, then everything goes: the
//!   files, the Secure Enclave keys, and the Keychain items (floor,
//!   `peer_endpoint`) — the same sweep as an unfinished pairing. The engine
//!   is left UNINITIALIZED, ready to pair again.
//!
//! Phone only: the Mac's vault is never removed this way.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::{lock_core, Deps, LockReason, OpOutcome, VaultCore};
use crate::errors::ErrorCode;
use crate::state::VaultState;
use crate::storage::revision_rows::all_rows;
use crate::storage::sources::{self, Source};
use crate::storage::VaultStore;

const REASON: &str = "SOURCE Vault: remove this vault from this iPhone";

/// Revisions only this device holds: never confirmed by the provider.
fn local_only(store: &VaultStore) -> Result<usize, ErrorCode> {
    let mut n = 0;
    for r in all_rows(&store.conn)? {
        if !sources::of(&store.conn, &r.revision_id)?.contains(&Source::Provider) && !crate::storage::set_aside::refused(&store.conn, &r.revision_id)? {
            n += 1;
        }
    }
    Ok(n)
}

pub fn preview(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    let c = lock_core(core);
    if !c.phone {
        return OpOutcome::err(ErrorCode::UnknownOp);
    }
    if c.state == VaultState::Uninitialized {
        return OpOutcome::err(ErrorCode::BadState);
    }
    let state = c.state_answer();
    let count = match c.store.as_ref() {
        Some(s) => match local_only(s) {
            Ok(n) => json!(n),
            Err(e) => return OpOutcome::err(e),
        },
        None => Value::Null,
    };
    OpOutcome::ok(json!({
        "local_only": count,
        "unknown": count.is_null(),
        "access_lost": state.get("access_lost").is_some(),
        "removed": state.get("removed").cloned(),
    }))
}

pub fn remove(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    {
        let c = lock_core(core);
        if !c.phone {
            return OpOutcome::err(ErrorCode::UnknownOp);
        }
        if c.state == VaultState::Uninitialized || !matches!(c.state, VaultState::Locked | VaultState::Unlocked) {
            return OpOutcome::err(ErrorCode::BadState);
        }
    }
    if frame.get("confirm").and_then(Value::as_bool) != Some(true) {
        return OpOutcome::err(ErrorCode::InvalidInput);
    }
    if !deps.la.check(REASON) {
        return OpOutcome::err(ErrorCode::PresenceDenied);
    }
    let mut c = lock_core(core);
    let dir = c.vault_dir.clone();
    for e in c.lock(LockReason::Explicit) {
        deps.events.emit(e);
    }
    c.store = None;
    super::join_ops::discard(&dir);
    *c = VaultCore::boot_phone(dir);
    deps.events.emit(super::ev_state(c.state));
    OpOutcome::ok(json!({}))
}
