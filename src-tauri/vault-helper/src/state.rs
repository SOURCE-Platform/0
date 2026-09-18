//! Vault state machine, Phase A subset (spec §13.1).
//!
//! The full spec state machine has eleven states; Phase A can only ever be
//! in `Uninitialized` (no vault on this machine) or `Locked` (a vault header
//! exists and the helper holds no keys — the boot state, and the only state
//! a helper restart may produce, spec §1.6). Later phases add the remaining
//! states and the transitions between them; nothing here anticipates them
//! beyond the enum documentation.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::VAULT_HEADER_NAME;

/// Phase A states (subset of spec §13.1; later phases add Unlocking,
/// Unlocked, Relocking, Authorizing, Recovering, Merging, Compromised,
/// Exporting, Upgrading).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VaultState {
    /// No vault exists on this machine (spec §13.1).
    Uninitialized,
    /// Vault exists; helper holds no key material. The only state a helper
    /// (re)start may produce when a vault is present (spec §1.6).
    Locked,
}

impl VaultState {
    pub fn as_str(self) -> &'static str {
        match self {
            VaultState::Uninitialized => "uninitialized",
            VaultState::Locked => "locked",
        }
    }
}

/// Boot-time state detection (spec §1.6): a vault is present iff the vault
/// directory contains `header.json` (spec §3.1). Absent directory or absent
/// header means UNINITIALIZED. Any I/O error is treated as "no header":
/// mis-detection toward UNINITIALIZED is the safe direction in Phase A
/// because no op here can mutate vault data.
pub fn detect_boot_state(vault_dir: &Path) -> VaultState {
    if vault_dir.join(VAULT_HEADER_NAME).is_file() {
        VaultState::Locked
    } else {
        VaultState::Uninitialized
    }
}

/// Apply the `lock` op (spec §1.5: "immediate lock, any state").
///
/// Phase A semantics: LOCKED stays LOCKED; UNINITIALIZED has nothing to
/// lock and stays UNINITIALIZED (there is no vault to lock). Returns the
/// resulting state. The transition is idempotent, matching spec §13.2.
pub fn apply_lock(state: VaultState) -> VaultState {
    state // both current states: lock is a no-op identity transition
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_state_is_uninitialized_without_header() {
        let dir = std::env::temp_dir().join(format!("vh-state-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(detect_boot_state(&dir), VaultState::Uninitialized);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn boot_state_is_locked_with_header() {
        let dir = std::env::temp_dir().join(format!("vh-state-h-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(VAULT_HEADER_NAME), b"{}").unwrap();
        assert_eq!(detect_boot_state(&dir), VaultState::Locked);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lock_is_idempotent_and_never_unlocks() {
        assert_eq!(apply_lock(VaultState::Locked), VaultState::Locked);
        assert_eq!(
            apply_lock(VaultState::Uninitialized),
            VaultState::Uninitialized
        );
    }
}
