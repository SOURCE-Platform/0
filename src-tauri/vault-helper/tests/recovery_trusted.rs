//! RC-05…RC-07 (§12 scenarios 5–7), BK-10 and BK-16 against
//! FsBackupStore: a trusted device survives and repairs MP / RK.

mod recovery_fx;

use recovery_fx::*;
use vault_helper::backup::fs_store::RecoveryKind;
use vault_helper::backup::manifest::SignedManifest;
use vault_helper::backup::snapshot;
use vault_helper::crypto::kdf::{self, Argon2Params};
use vault_helper::crypto::wrap::{self, RecoveryWrapFile};
use vault_helper::recovery::creds;
use vault_helper::recovery::total_loss::Credential;
use vault_helper::storage::VaultStore;
use vault_helper::vault::recovery_ops;

/// RC-05 + BK-16 (MP): new MP on the trusted device, no rotation; the MP
/// locator + credential are re-registered; the old MP stops working with
/// no window where both work.
#[test]
fn rc05_mp_forgotten_trusted_device_retained() {
    let w = world();
    let mut store = VaultStore::open(&w.a_dir).unwrap();
    let vk_gen = store.header.vk_generation;
    let salt = recovery_ops::set_master_password(&mut store, &w.vk, MP_NEW).unwrap();
    assert_eq!(store.header.vk_generation, vk_gen, "MP change does not rotate VK");
    assert!(recovery_ops::prove_mp(&store, MP).is_err(), "old MP dead locally");
    recovery_ops::prove_mp(&store, MP_NEW).unwrap();
    // Publish + re-register (the §12 step-2 provider calls).
    snapshot::publish(&w.backup, &store, &w.registry, Some(&w.manifest), &w.mac, w.auth_mac()).unwrap();
    let pk = kdf::derive_pk(MP_NEW, &salt, Argon2Params::V1).unwrap();
    let c = creds::mp_creds(&pk, &store.header.locator_salt_mp.0).unwrap();
    w.backup.update_kdf_salt(&w.vault_id, salt, w.auth_mac()).unwrap();
    w.backup.register_recovery(&w.vault_id, RecoveryKind::Mp, &c.locator, c.cred.expose(), w.auth_mac()).unwrap();
    assert!(unwraps_current(&w, Credential::Mp(MP)).is_err(), "old MP recovery credential fails");
    assert_eq!(unwraps_current(&w, Credential::Mp(MP_NEW)).unwrap().expose(), w.vk.expose());
    assert_eq!(unwraps_current(&w, Credential::Rk(&w.rk)).unwrap().expose(), w.vk.expose(), "RK unaffected");
    w.cleanup(&[]);
}

/// RC-06 / RC-07 + BK-10 + BK-16 (RK): new RK + VK rotation on the trusted
/// device; old RK fails on current state; the retained pre-rotation
/// snapshot still decrypts with the old RK (the documented limitation).
fn rk_replacement(incident: bool) {
    let w = world();
    let old_manifest = w.manifest.clone();
    let store = VaultStore::open(&w.a_dir).unwrap();
    let pk = recovery_ops::prove_mp(&store, MP).unwrap();
    let out = recovery_ops::rotate_recovery_key(store, &w.vk, &pk).unwrap();
    let new_vk = out.rotation.new_vk;
    let store = VaultStore::open(&w.a_dir).unwrap();
    assert_eq!(store.header.vk_generation, out.rotation.vk_generation);
    let head = snapshot::publish(&w.backup, &store, &w.registry, Some(&old_manifest), &w.mac, w.auth_mac()).unwrap();
    let c = creds::rk_creds(&out.new_rk, &store.header.locator_salt_rk.0).unwrap();
    w.backup.register_recovery(&w.vault_id, RecoveryKind::Rk, &c.locator, c.cred.expose(), w.auth_mac()).unwrap();

    // Current state: old RK refused; new RK and MP open the new VK only.
    assert!(unwraps_current(&w, Credential::Rk(&w.rk)).is_err(), "old RK refused on current state");
    assert_eq!(unwraps_current(&w, Credential::Rk(&out.new_rk)).unwrap().expose(), new_vk.expose());
    assert_eq!(unwraps_current(&w, Credential::Mp(MP)).unwrap().expose(), new_vk.expose());
    // BK-10: the retained previous generation (§11.3 retention 2) is a
    // historical snapshot the old RK still opens.
    let retained = w.backup.retained_manifests(&w.vault_id).unwrap();
    let hist = SignedManifest::decode(&retained[0]).unwrap();
    assert_eq!(hist.generation, old_manifest.generation);
    assert!(head.generation > hist.generation);
    let d = snapshot::download(&w.backup, &retained[0], w.auth_mac()).unwrap();
    let f: RecoveryWrapFile = serde_json::from_slice(d.wrap_rk.as_ref().unwrap()).unwrap();
    let hist_vk = wrap::open_wrap_rk(&f, &w.rk, &w.vault_id).expect("old RK + old wrap still decrypts").vk;
    assert_eq!(hist_vk.expose(), w.vk.expose());
    let snap_dir = tmp("hist");
    let hist_store = snapshot::materialize(&snap_dir, &d).unwrap();
    assert!(hist_store.read_tip(&hist_vk, &w.refs[1]).is_ok(), "historical bytes decrypt");
    if incident {
        // Scenario 7 adds product copy + retention; the mechanics are the
        // same. Retention: the pre-rotation manifest is still served.
        assert_eq!(retained.len(), 1);
    }
    w.cleanup(&[&snap_dir]);
}

#[test]
fn rc06_rk_lost_trusted_device_retained() {
    rk_replacement(false);
}

#[test]
fn rc07_rk_suspected_stolen() {
    rk_replacement(true);
}
