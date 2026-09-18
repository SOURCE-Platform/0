//! Vault state machine (spec §13.1), Phase C subset.
//!
//! Phase C adds UNLOCKING (panel/unwrap in flight), UNLOCKED (VK resident
//! in this process only), AUTHORIZING (per-op presence substate; VK
//! stays resident) and ERROR (fatal vault data problem; zeroized on
//! entry). RECOVERING / ROTATING_KEYS / SYNCING / BACKING_UP /
//! COMPROMISED land with Phases D/E/F.
//!
//! Transition rules (§13.2/§13.3):
//! - any unexpected op for the current state → BAD_STATE, no side effects;
//! - every transition into Locked/Error zeroizes key material (handled by
//!   the ops layer dropping the resident secrets);
//! - process death in any state → LOCKED on next start (nothing key-like
//!   is ever persisted outside wraps).

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::VAULT_HEADER_NAME;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VaultState {
    /// No vault exists on this machine.
    Uninitialized,
    /// Vault exists; helper holds no key material (the only boot state
    /// with a vault present, §1.6).
    Locked,
    /// MP entry/unwrap in flight (§13.1). No other op proceeds.
    Unlocking,
    /// VK resident; ops per §13.2.
    Unlocked,
    /// One op's presence check in flight; VK resident (§13.1 substate).
    Authorizing,
    /// Fatal vault-data problem; only get_state/lock proceed (§13.2).
    Error,
}

impl VaultState {
    pub fn as_str(self) -> &'static str {
        match self {
            VaultState::Uninitialized => "uninitialized",
            VaultState::Locked => "locked",
            VaultState::Unlocking => "unlocking",
            VaultState::Unlocked => "unlocked",
            VaultState::Authorizing => "authorizing",
            VaultState::Error => "error",
        }
    }

    /// Whether VK is resident in this state (§13.2 table).
    pub fn vk_resident(self) -> bool {
        matches!(
            self,
            VaultState::Unlocked | VaultState::Authorizing | VaultState::Unlocking
        )
    }
}

/// Boot-time state detection (§1.6): a vault is present iff the vault
/// directory contains `header.json` (§3.1). Header *contents* are not
/// trusted at boot — corrupt/future headers are surfaced when the unlock
/// path runs the §3.6 open sequence.
pub fn detect_boot_state(vault_dir: &Path) -> VaultState {
    if vault_dir.join(VAULT_HEADER_NAME).is_file() {
        VaultState::Locked
    } else {
        VaultState::Uninitialized
    }
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
    fn vk_residency_matches_state_table() {
        assert!(!VaultState::Locked.vk_resident());
        assert!(!VaultState::Uninitialized.vk_resident());
        assert!(!VaultState::Error.vk_resident());
        assert!(VaultState::Unlocked.vk_resident());
        assert!(VaultState::Authorizing.vk_resident());
        assert!(VaultState::Unlocking.vk_resident());
    }
}
