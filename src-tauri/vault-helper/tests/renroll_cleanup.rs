//! Final review (core findings 1 and 6): a Mac join's cleanup deletes only
//! the keys that attempt created, never a vault's own; a join in progress
//! keeps setup and recovery away; a lock ends the attempt with its keys.
//! Synthetic data only.

mod vault_fx;

use serde_json::json;
use vault_fx::*;

fn begin(fx: &Fx) -> serde_json::Value {
    fx.op(json!({"op": "renroll_begin", "fp": "11".repeat(32), "name": "Test Mac"}))
}

/// RE-08: a planted attempt next to an existing vault removes nothing of it.
#[test]
fn a_planted_staging_never_costs_the_vault_its_keys() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    // A same-user process plants a staging copy naming the vault's keys.
    let staging = fx.dir.join("renroll");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::copy(fx.dir.join("device.json"), staging.join("device.json")).unwrap();
    vault_helper::vault::renroll_ops::wipe_staging(&fx.dir, None);
    assert!(!staging.exists(), "the staging is gone");
    vault_helper::vault::renroll_ops::recover_at_boot(&fx.dir);
    assert!(vault_helper::device::identity::SeDevice::load(&fx.dir).is_ok(), "the vault's own keys survive");
    // Even naming the vault's tag outright deletes nothing of the root's.
    std::fs::create_dir_all(&staging).unwrap();
    let tag = vault_helper::device::identity::read_file(&fx.dir).unwrap().key_tag;
    vault_helper::vault::renroll_ops::wipe_staging(&fx.dir, Some(&tag));
    assert!(vault_helper::device::identity::SeDevice::load(&fx.dir).is_ok());
    fx.remove_dir();
}

#[test]
fn a_join_in_progress_holds_setup_and_recovery_and_a_lock_ends_it() {
    let _g = serial();
    let fx = fx();
    let b = begin(&fx);
    assert_eq!(b["ok"], true, "{b}");
    let staged = vault_helper::device::identity::read_file(&fx.dir.join("renroll")).unwrap().key_tag;
    assert_eq!(fx.op(json!({"op": "setup_vault", "handle": "fixture-handle"}))["error"], "BAD_STATE");
    assert_eq!(fx.op(json!({"op": "recovery_begin", "kind": "mp", "locate_response": "{}"}))["error"], "BAD_STATE");
    fx.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    assert!(!fx.dir.join("renroll").exists(), "a lock sweeps the staging");
    assert!(vault_helper::device::se::signing_public(&staged).is_err(), "and the attempt's keys");
    fx.remove_dir();
}
