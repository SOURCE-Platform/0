//! RC-03 / RC-04 (§12 scenarios 3–4) + RF-01 / RF-08 + FR-01 against
//! FsBackupStore: both devices lost, recovery on a fresh device in the
//! corrected order, no post-finalize rotation. Repeated recovery and the
//! §4.8 registry checkpoint live in `registry_checkpoint.rs`.

mod recovery_fx;

use recovery_fx::*;
use vault_helper::backup::fs_store::Auth;
use vault_helper::backup::manifest::SignedManifest;
use vault_helper::backup::snapshot;
use vault_helper::crypto::record::{self, RecordCiphertext};
use vault_helper::crypto::secret::SecretBytes;
use vault_helper::crypto::wrap::{self, RecoveryWrapFile};
use vault_helper::errors::ErrorCode;
use vault_helper::recovery::total_loss::{begin, CompletePlan, Credential};
use vault_helper::registry::chain::verify_chain;
use vault_helper::registry::device::{DeviceIdentity, SoftwareDevice, PLATFORM_MACOS};
use vault_helper::storage::revisions::uuid_bytes;
use vault_helper::storage::VaultStore;

fn plan<'a>() -> CompletePlan<'a> {
    CompletePlan { new_mp: None, keep_rk: None, fault: None, on_body: None }
}

/// RF-08 + "old VK fails": every record object the new head references
/// opens only under the fresh VK.
fn assert_head_sealed_only_under(w: &World, fresh: &SecretBytes<32>, auth: Auth<'_>) {
    let head = SignedManifest::decode(&w.backup.head_manifest(&w.vault_id, auth).unwrap().unwrap()).unwrap();
    let d = snapshot::download(&w.backup, &head.encode(), auth).unwrap();
    assert!(!d.rows.is_empty());
    for row in &d.rows {
        let rid = uuid_bytes(&row.record_id).unwrap();
        let sealed = RecordCiphertext { nonce: row.nonce, ct: row.ct.clone() };
        let open = |vk: &SecretBytes<32>| record::open_record(vk, &w.vault_id, &rid, row.schema_version, row.vk_generation, &sealed);
        assert!(open(fresh).is_ok(), "record opens under fresh VK");
        assert!(open(&w.vk).is_err(), "old VK → INTEGRITY_FAILURE");
        assert_eq!(row.vk_generation, head.vk_generation);
    }
}

#[test]
fn rc03_mp_recovery_both_devices_lost() {
    let w = world();
    let before = contents(&VaultStore::open(&w.a_dir).unwrap(), &w.vk);
    let old = w.manifest.clone();

    let session = begin(&w.backup, EMAIL, Credential::Mp(MP)).unwrap();
    // FR-01: the freshness preview exists before completion.
    let p = session.preview();
    assert_eq!((p.vault_id, p.generation, p.created_at), (w.vault_id, old.generation, old.created_at));
    assert_eq!(p.item_count, 2);

    let c_dir = tmp("recovered");
    let newdev = SoftwareDevice::generate("Replacement Mac", PLATFORM_MACOS);
    let out = session
        .complete(&c_dir, &newdev, CompletePlan { keep_rk: Some(&w.rk), ..plan() })
        .unwrap();
    assert!(out.new_rk.is_none(), "user-supplied RK kept");

    // RF-08: exactly one rotation, before finalize; nothing after it.
    assert_eq!(out.vk_generation, old.vk_generation + 1);
    assert_eq!(out.generation, old.generation + 1);
    let local = VaultStore::open(&c_dir).unwrap();
    assert_eq!(local.header.vk_generation, out.vk_generation, "no post-finalize rotation");
    assert_ne!(out.vk.expose(), w.vk.expose());
    // Contents equal (RC pattern).
    assert_eq!(contents(&local, &out.vk), before);

    // RF-01: head advanced, new credential active, registry extended.
    let c_auth = Auth::Device { device_id: newdev.device_id(), cred: &out.device_cred };
    let head_bytes = w.backup.head_manifest(&w.vault_id, c_auth).expect("new device cred active").unwrap();
    let head = SignedManifest::decode(&head_bytes).unwrap();
    assert_eq!(head.prev_manifest_hash, old.hash());
    assert_eq!(head.signer_device_id, newdev.device_id());
    assert_head_sealed_only_under(&w, &out.vk, c_auth);
    let d = snapshot::download(&w.backup, &head_bytes, c_auth).unwrap();
    // A surviving device that knew the old VK verifies the epoch itself.
    let ctx = KnownVk { manifest_hash: old.hash(), vk: *w.vk.expose(), superseded: vec![] };
    let st = verify_chain(&d.registry, &w.vault_id, &ctx).unwrap();
    assert_eq!(st.epoch, 1);
    assert!(st.active_device(&newdev.device_id()).is_some());

    // Wraps: same MP and the kept RK both open exactly the fresh VK.
    let rk_file: RecoveryWrapFile = serde_json::from_slice(d.wrap_rk.as_ref().unwrap()).unwrap();
    assert_eq!(wrap::open_wrap_rk(&rk_file, &w.rk, &w.vault_id).unwrap().vk.expose(), out.vk.expose());
    let again = unwraps_current(&w, Credential::Mp(MP)).expect("same MP still opens the new state");
    assert_eq!(again.expose(), out.vk.expose());
    w.cleanup(&[&c_dir]);
}

#[test]
fn rc03b_mp_only_recovery_must_issue_a_new_rk() {
    let w = world();
    let c_dir = tmp("recovered");
    let newdev = SoftwareDevice::generate("Replacement Mac", PLATFORM_MACOS);
    let out = begin(&w.backup, EMAIL, Credential::Mp(MP)).unwrap().complete(&c_dir, &newdev, plan()).unwrap();
    let new_rk = out.new_rk.expect("RK cannot be kept without RK_bytes → new RK issued");
    assert!(unwraps_current(&w, Credential::Rk(&w.rk)).is_err(), "old RK dead (locator re-registered)");
    let vk = unwraps_current(&w, Credential::Rk(&new_rk)).expect("new RK opens the new state");
    assert_eq!(vk.expose(), out.vk.expose());
    w.cleanup(&[&c_dir]);
}

#[test]
fn rc04_rk_recovery_sets_new_mp() {
    let w = world();
    let before = contents(&VaultStore::open(&w.a_dir).unwrap(), &w.vk);
    let session = begin(&w.backup, EMAIL, Credential::Rk(&w.rk)).unwrap();
    let c_dir = tmp("recovered");
    let newdev = SoftwareDevice::generate("Replacement Mac", PLATFORM_MACOS);
    // The RK path cannot re-seal the MP wrap without a (new) MP.
    let missing = begin(&w.backup, EMAIL, Credential::Rk(&w.rk)).unwrap().complete(&tmp("x"), &newdev, plan());
    assert_eq!(missing.err(), Some(ErrorCode::InvalidInput));
    let out = session.complete(&c_dir, &newdev, CompletePlan { new_mp: Some(MP_NEW), ..plan() }).unwrap();
    assert!(out.new_rk.is_none(), "entered RK is kept");
    assert_eq!(contents(&VaultStore::open(&c_dir).unwrap(), &out.vk), before);
    assert!(unwraps_current(&w, Credential::Mp(MP)).is_err(), "old MP dead");
    assert_eq!(unwraps_current(&w, Credential::Mp(MP_NEW)).unwrap().expose(), out.vk.expose());
    assert_eq!(unwraps_current(&w, Credential::Rk(&w.rk)).unwrap().expose(), out.vk.expose());
    let c_auth = Auth::Device { device_id: newdev.device_id(), cred: &out.device_cred };
    assert_head_sealed_only_under(&w, &out.vk, c_auth);
    w.cleanup(&[&c_dir]);
}

#[test]
fn wrong_credentials_yield_no_decryption_result() {
    let w = world();
    assert_eq!(begin(&w.backup, EMAIL, Credential::Mp(b"synthetic-wrong-password")).err(), Some(ErrorCode::WrongCredential));
    let bogus = vault_helper::crypto::secret::random_secret();
    assert_eq!(begin(&w.backup, EMAIL, Credential::Rk(&bogus)).err(), Some(ErrorCode::WrongCredential));
    assert_eq!(begin(&w.backup, "nobody@example.test", Credential::Mp(MP)).err(), Some(ErrorCode::WrongCredential));
    w.cleanup(&[]);
}
