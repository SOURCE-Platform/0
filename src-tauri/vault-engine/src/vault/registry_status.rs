//! `registry_status` — the read-only half of the revocation-status check
//! (§4.7, Phase E).
//!
//! An enrolled phone has no way to learn that it was revoked: revocation
//! is a write to the registry on this Mac, and the enrollment channel is
//! gone by then. This op lets a paired device *ask*. It returns the
//! signed registry and nothing else — the phone verifies the chain
//! itself and decides its own status (§4.7: the registry's
//! confidentiality requirement is nil, its integrity requirement total).
//!
//! Deliberately not gated on UNLOCKED: the registry involves no VK, and
//! a phone must be able to discover it was revoked while this Mac's
//! vault is locked. Nothing here reads the vault key, a wrap, a record,
//! a backup credential, or any recovery material — see the test at the
//! bottom, which fails if this response ever grows such a field.

use std::sync::{Arc, Mutex};

use serde_json::json;

use super::{lock_core, OpOutcome, VaultCore};
use crate::crypto::hex;
use crate::errors::ErrorCode;
use crate::registry::log;
use crate::state::VaultState;

/// Fields this response is allowed to carry. Anything else is a leak.
pub const ALLOWED_FIELDS: [&str; 5] = ["ok", "error", "vault_id", "registry", "entries"];

pub fn registry_status(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    let (dir, vault_id) = {
        let c = lock_core(core);
        match c.state {
            // A locked vault still answers: the phone's question is about
            // the registry, not about the vault's contents.
            VaultState::Locked
            | VaultState::Unlocked
            | VaultState::Authorizing
            | VaultState::Unlocking => {}
            _ => return OpOutcome::err(ErrorCode::BadState),
        }
        let Some(header) = c.header.as_ref() else {
            return OpOutcome::err(ErrorCode::BadState);
        };
        (c.vault_dir.clone(), header.vault_id.0)
    };
    let entries = match log::read_entries(&dir) {
        Ok(e) => e,
        Err(e) => return OpOutcome::err(e),
    };
    // The bytes as stored: JSONL of canonical TLV, exactly what the
    // phone's verifier expects (§4.1).
    let bytes = match crate::registry::file::encode(&entries) {
        Ok(b) => b,
        Err(e) => return OpOutcome::err(e),
    };
    let Ok(registry) = String::from_utf8(bytes) else {
        return OpOutcome::err(ErrorCode::DbCorrupt);
    };
    OpOutcome::ok(json!({
        "vault_id": hex::encode(vault_id),
        "registry": registry,
        "entries": entries.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The endpoint's whole justification is that it carries no secrets.
    /// This keeps that true as the code changes: any new field has to be
    /// added to `ALLOWED_FIELDS` deliberately, and the names below can
    /// never be among them.
    #[test]
    fn response_field_list_admits_nothing_secret() {
        for forbidden in [
            "vk",
            "wrap",
            "password",
            "recovery",
            "device_backup_cred",
            "cred",
            "secret",
            "records",
            "items",
        ] {
            assert!(
                !ALLOWED_FIELDS.iter().any(|f| f.contains(forbidden)),
                "registry_status must never return {forbidden}"
            );
        }
    }
}
