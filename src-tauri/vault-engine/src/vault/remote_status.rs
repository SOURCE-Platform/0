//! Remote-completion status for the UI (spec v0.4 §11.3.2, §15): what
//! is still `REMOTE_UPDATE_PENDING`, whether it is security-driven (the
//! persistent "not yet cut off at your backup" banner), whether a
//! revocation has reached `BACKUP_REVOCATION_FAILED`, and whether the
//! user must redo a change (`needs_user`). Non-secret; readable while
//! LOCKED (the DB is ciphertext-only). `remote_update_status` answers it
//! on request; `backup_commit_result` emits it as the `remote_update`
//! event.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::{lock_core, OpOutcome, VaultCore};
use crate::errors::ErrorCode;
use crate::storage::VaultStore;
use crate::sync::pending::{self, PendingOp, PendingRemote};

/// After this many failed attempts a pending revocation is surfaced as
/// `BACKUP_REVOCATION_FAILED`; retries continue (§11.3.2).
pub const REVOCATION_FAILED_AFTER: u32 = 3;

pub fn summary(p: Option<&PendingRemote>) -> Value {
    let Some(p) = p else {
        return json!({ "pending": false });
    };
    let ops = p.all_ops();
    let revocation = ops.contains(&PendingOp::Revocation);
    json!({
        "pending": true,
        "ops": ops,
        "security_driven": p.security(),
        "needs_user": p.needs_redo(),
        "attempts": p.attempts,
        "last_error": p.last_error,
        "revocation_failed": revocation && p.attempts >= REVOCATION_FAILED_AFTER,
    })
}

/// The summary plus a security change lost with a restored older copy
/// (§22.14), which the user must redo.
pub fn status_of(store: &VaultStore) -> Result<Value, ErrorCode> {
    let mut s = summary(pending::load(&store.conn)?.as_ref());
    if let Some(lost) = crate::storage::kv::get::<Value>(&store.conn, super::floor::LOST_KEY)? {
        s["lost_change"] = lost;
    }
    Ok(s)
}

/// The `remote_update` event for the current record.
pub fn event(store: &VaultStore) -> Result<Value, ErrorCode> {
    let s = status_of(store)?;
    let status = if s["pending"] == true { "remote_update_pending" } else { "remote_committed" };
    Ok(json!({ "event": "remote_update", "status": status, "detail": s }))
}

/// `remote_update_status` → the summary (no vault → `{pending:false}`).
pub fn remote_update_status(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    let c = lock_core(core);
    let run = || -> Result<Value, ErrorCode> {
        let opened;
        let store = match c.store.as_ref() {
            Some(s) => s,
            None if c.vault_dir.join(crate::VAULT_HEADER_NAME).exists() => {
                opened = VaultStore::open(&c.vault_dir)?;
                &opened
            }
            None => return Ok(summary(None)),
        };
        status_of(store)
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::header::{Hex16, Hex32};
    use crate::sync::pending::Base;

    fn rec(ops: Vec<PendingOp>, attempts: u32) -> PendingRemote {
        let base = Base { vk_generation: 1, kdf_salt: Hex16([0; 16]), auth_salt_mp: Hex16([0; 16]), auth_salt_rk: Hex16([0; 16]), registry_head: Hex32([0; 32]) };
        PendingRemote { ops, security_driven: true, local_committed_at: 0, recovery_auth_updates: Vec::new(), base, needs_user: false, attempts, last_error: None, version: 1, in_flight: Vec::new(), awaiting_redo: Vec::new(), awaiting_security: false, target_device_ids: Vec::new(), awaiting_redo_targets: Vec::new() }
    }

    /// RU-04: a pending revocation surfaces BACKUP_REVOCATION_FAILED after
    /// three failed attempts and stays pending; other ops never do.
    #[test]
    fn revocation_failed_after_three_attempts() {
        assert_eq!(summary(None), json!({ "pending": false }));
        assert_eq!(summary(Some(&rec(vec![PendingOp::Revocation], 2)))["revocation_failed"], false);
        let s = summary(Some(&rec(vec![PendingOp::Revocation], 3)));
        assert_eq!((s["pending"].clone(), s["revocation_failed"].clone()), (json!(true), json!(true)));
        assert_eq!(summary(Some(&rec(vec![PendingOp::RkReplacement], 9)))["revocation_failed"], false);
        assert_eq!(s["ops"], json!(["revocation"]));
    }
}
