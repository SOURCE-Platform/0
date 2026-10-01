//! Review of `30539be` (SEC-B1/B2/B4): the vault-key commitment commits
//! in the same journal as the key it vouches for; an unpublished security
//! change that disappears is reported at the next unlock; before any
//! provider state, a rewritten registry is not trusted. Synthetic only.

mod vault_fx;

use serde_json::json;
use vault_fx::*;
use vault_helper::storage::VaultStore;
use vault_helper::vault::vk_commit::{self, Verdict};

fn lock(fx: &Fx) {
    fx.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
}

fn take(fx: &Fx) -> (VaultStore, vault_helper::crypto::secret::SecretBytes<32>) {
    let mut c = fx.core.lock().unwrap();
    (c.store.take().unwrap(), c.vk.take().unwrap())
}

/// SEC-B1: a rotation through the library alone (no op-level
/// `commit_resident` afterwards) leaves a commitment that verifies for
/// the new key — it was staged in the rotation journal.
#[test]
fn a_rotation_commits_its_key_commitment() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let (store, vk) = take(&fx);
    let pk = vault_helper::vault::recovery_ops::prove_mp(&store, &vk, MP).unwrap();
    let rot = vault_helper::vault::recovery_ops::rotate_recovery_key(store, &vk, &pk, false).unwrap();
    let h = VaultStore::read_header(&fx.dir).unwrap();
    assert_eq!(h.vk_generation, 2);
    let v = vk_commit::verify(&fx.dir, &h.vault_id.0, h.vk_generation, &rot.rotation.new_vk).unwrap();
    assert!(matches!(v, Verdict::Committed));
    assert!(vk_commit::verify(&fx.dir, &h.vault_id.0, h.vk_generation, &vk).is_err(), "the old key no longer verifies");
    let mut fx = fx;
    fx.reboot(); // the core's cached header predates the library rotation
    assert_eq!(unlock(&fx, MP)["ok"], true, "unlocks on the rotated key");
    fx.remove_dir();
}

/// SEC-B2: a pending security change whose record disappears (deleted,
/// or an edited copy) is reported at the next unlock, and redoing it
/// clears only that warning.
#[test]
fn a_vanished_pending_change_is_reported_at_unlock() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    fx.push_panel(submitted(MP));
    assert_eq!(fx.op(json!({"op": "rotate_recovery_key", "suspected_theft": true}))["ok"], true);
    lock(&fx);
    {
        let store = VaultStore::open(&fx.dir).unwrap();
        vault_helper::sync::pending::clear(&store.conn).unwrap(); // the attacker's edit
    }
    assert_eq!(unlock(&fx, MP)["ok"], true);
    let s = fx.op(json!({"op": "remote_update_status"}));
    assert!(s["lost_change"].as_array().unwrap().iter().any(|o| o == "rk_replacement"), "{s}");
    // Redoing it answers the warning.
    fx.push_panel(submitted(MP));
    assert_eq!(fx.op(json!({"op": "rotate_recovery_key", "suspected_theft": true}))["ok"], true);
    // (The deletion also took the never-committed `vault_create`, which
    // stays reported: that loss is real too.)
    let after = fx.op(json!({"op": "remote_update_status"}));
    let lost = after["lost_change"].as_array().cloned().unwrap_or_default();
    assert!(!lost.iter().any(|o| o == "rk_replacement"), "{after}");
    assert!(lost.iter().any(|o| o == "vault_create"), "{after}");
    fx.remove_dir();
}

/// SEC-B4: before any provider state exists the floor anchors on this
/// vault's genesis; a registry rewritten from a different genesis never
/// opens with authority.
#[test]
fn a_rewritten_registry_before_the_first_commit_is_not_trusted() {
    use vault_helper::registry::device::{SoftwareDevice, PLATFORM_MACOS};
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    lock(&fx);
    let other = SoftwareDevice::generate("Someone else's genesis", PLATFORM_MACOS);
    let genesis = vault_proto::registry::build::genesis(&other).unwrap();
    vault_helper::registry::log::write_all(&fx.dir, &[genesis]).unwrap();
    let r = unlock(&fx, MP);
    let open_with_authority = r["ok"] == true && !fx.core.lock().unwrap().behind;
    assert!(!open_with_authority, "{r}");
    fx.remove_dir();
}
