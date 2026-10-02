//! Debug-only test namespace (spec v0.4 §18 Phase F pre-gate).
//!
//! Test and gate runs must be unattended and must never touch the user's
//! real vault items, and they must not leave synthetic Keychain items
//! behind for a later run to trip over. The Phase E suites named their
//! items by process id and deleted them only at fixture start, so a later
//! process that reused a PID found an item whose ACL trusts a different
//! binary, and macOS raised a login-password prompt mid-gate.
//!
//! This module fixes all three:
//! - a random per-run namespace (`ov0t-<run_id>-` for Keychain services,
//!   `test.<run_id>.` for Secure Enclave key tags) instead of PIDs;
//! - teardown deletion of the namespace's synthetic items;
//! - a fail-fast guard: interactive Keychain prompts are disabled for the
//!   test process, so a would-be prompt fails the test instead of hanging
//!   (verified: a foreign-ACL read then returns `errSecAuthFailed`).
//!
//! Compiled out of release builds; production ACLs are untouched.

use std::sync::OnceLock;

use crate::crypto::hex;

const STATE_SERVICE: &str = "com.racker.zero.vault.state";
const PREFS_SERVICE: &str = "com.racker.zero.vault.helper-prefs";

/// 16 hex chars from the OS RNG, fixed for the life of the process.
pub fn run_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).expect("OS RNG");
        hex::encode(b)
    })
}

/// The Keychain service prefix for this run.
pub fn keychain_prefix() -> String {
    format!("ov0t-{}-", run_id())
}

/// The Secure Enclave key-tag prefix for this run.
pub fn se_tag_prefix() -> String {
    format!("test.{}.", run_id())
}

/// Install the run namespace for this process (idempotent) and disable
/// interactive Keychain prompts. Returns the Keychain prefix.
pub fn init_test_namespace() -> String {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        std::env::set_var("OV0_VAULT_KEYCHAIN_PREFIX", keychain_prefix());
        std::env::set_var("OV0_VAULT_SE_TAG_PREFIX", se_tag_prefix());
        crate::keychain::disable_user_interaction();
    });
    keychain_prefix()
}

/// Delete this run's synthetic Keychain items (rollback state and prefs).
pub fn wipe_test_keychain() {
    crate::keychain::delete_item(STATE_SERVICE);
    crate::keychain::delete_item(PREFS_SERVICE);
}
