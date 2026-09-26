//! §4.8 registry checkpoint (Phase D.1): repeated total-loss recovery and
//! the attacks the checkpoint must stop. Synthetic vaults/credentials.

mod recovery_fx;

use std::path::PathBuf;

use recovery_fx::*;
use vault_helper::backup::checkpoint::RegistryCheckpoint;
use vault_helper::backup::fs_store::Auth;
use vault_helper::backup::index_v1::{IndexRef, ObjectIndex};
use vault_helper::backup::manifest::SignedManifest;
use vault_helper::backup::snapshot;
use vault_helper::crypto::registry::{EntryKind, RegistryEntry};
use vault_helper::crypto::secret::{random_secret, SecretBytes};
use vault_helper::errors::ErrorCode;
use vault_helper::recovery::sheet::{SheetCheckpoint, SheetComparison};
use vault_helper::recovery::total_loss::{begin, CompletePlan, Credential, Recovered};
use vault_helper::registry::chain::{verify_chain, verify_chain_with, EpochPolicy};
use vault_helper::registry::device::{DeviceIdentity, SoftwareDevice, PLATFORM_MACOS};
use vault_helper::registry::file as registry_file;
use vault_helper::storage::VaultStore;

fn plan<'a>() -> CompletePlan<'a> {
    CompletePlan { new_mp: None, keep_rk: None, fault: None, on_body: None }
}

/// One full total-loss recovery with the master password.
fn recover(w: &World, tag: &str) -> (Recovered, SoftwareDevice, PathBuf) {
    let dir = tmp(tag);
    let dev = SoftwareDevice::generate("Replacement Mac", PLATFORM_MACOS);
    let out = begin(&w.backup, EMAIL, Credential::Mp(MP))
        .unwrap_or_else(|e| panic!("{tag}: begin failed {e:?}"))
        .complete(&dir, &dev, plan())
        .unwrap_or_else(|e| panic!("{tag}: complete failed {e:?}"));
    (out, dev, dir)
}

fn head_of(w: &World, auth: Auth<'_>) -> SignedManifest {
    SignedManifest::decode(&w.backup.head_manifest(&w.vault_id, auth).unwrap().unwrap()).unwrap()
}

/// The core Phase D.1 fix: a device that never held any historical VK
/// recovers, twice more, after earlier recovery epochs are in the chain.
#[test]
fn second_and_third_total_loss_recovery() {
    let w = world();
    let before = contents(&VaultStore::open(&w.a_dir).unwrap(), &w.vk);
    let (first, dev1, dir1) = recover(&w, "recover-1");
    let (second, dev2, dir2) = recover(&w, "recover-2");
    let (third, dev3, dir3) = recover(&w, "recover-3");

    // Each recovery rotates exactly once and advances the head by one.
    assert_eq!((first.vk_generation, second.vk_generation, third.vk_generation), (2, 3, 4));
    assert_eq!((first.generation, second.generation, third.generation), (2, 3, 4));
    // Contents survive all three.
    assert_eq!(contents(&VaultStore::open(&dir3).unwrap(), &third.vk), before);
    // Three epochs are in the chain; the third device is the live one.
    let auth = Auth::Device { device_id: dev3.device_id(), cred: &third.device_cred };
    let head = head_of(&w, auth);
    let d = snapshot::download(&w.backup, &head.encode(), auth).unwrap();
    let epochs = d.registry.iter().filter(|e| e.kind == EntryKind::RecoveryEpoch).count();
    assert_eq!(epochs, 3);
    let st = verify_chain_with(&d.registry, &w.vault_id, &EpochPolicy::CheckpointAnchored).unwrap();
    assert_eq!(st.epoch, 3);
    assert!(st.active_device(&dev3.device_id()).is_some());
    for gone in [dev1.device_id(), dev2.device_id()] {
        assert!(st.active_device(&gone).is_some(), "earlier devices stay installed (audit history)");
    }
    // The checkpoint the last recovery published binds exactly this state.
    d.checkpoint.verify_binding(&third.vk, &head, &head.registry_head, 3).unwrap();
    assert!(d.checkpoint.verify(&first.vk).is_err(), "superseded VK cannot MAC the current checkpoint");
    w.cleanup(&[&dir1, &dir2, &dir3]);
}

/// A provider that swaps in a registry of its own — with its own device
/// installed, and a manifest it can sign — cannot produce the checkpoint.
#[test]
fn provider_substitutes_a_different_registry() {
    let w = world();
    let (out, dev, dir) = recover(&w, "recover");
    let auth = Auth::Device { device_id: dev.device_id(), cred: &out.device_cred };
    let real = head_of(&w, auth);
    let d = snapshot::download(&w.backup, &real.encode(), auth).unwrap();

    // Forged registry: attacker genesis only. Structurally valid chain.
    let attacker = SoftwareDevice::generate("Provider", PLATFORM_MACOS);
    let forged: Vec<RegistryEntry> = vec![vault_helper::registry::build::genesis(&attacker).unwrap()];
    let reg_bytes = registry_file::encode(&forged).unwrap();
    let reg_key = vault_helper::backup::index_v1::meta_key("registry", &reg_bytes);
    w.backup.put_object(&w.vault_id, &reg_key, &reg_bytes, auth).unwrap();
    let mut idx = d.index.clone();
    idx.registry = IndexRef::of(reg_key, &reg_bytes);
    let mut m = real.clone();
    m.registry_head = vault_helper::crypto::registry::entry_hash(&forged[0]).unwrap();
    m.object_index_hash = idx.hash();
    let m = m.sign(&attacker).unwrap();
    w.backup.put_object(&w.vault_id, &ObjectIndex::key(m.generation), &idx.encode(), auth).unwrap();
    let key = vault_helper::backup::index_v1::meta_key("manifest", &m.encode());
    w.backup.put_object(&w.vault_id, &key, &m.encode(), auth).unwrap();
    // The provider serves its forged state but can only re-serve the real
    // checkpoint (it holds no VK).
    w.backup.put_object(&w.vault_id, &RegistryCheckpoint::key(m.generation), &d.checkpoint.encode(), auth).unwrap();
    w.backup.force_head(&w.vault_id, &m.encode()).unwrap();

    assert_eq!(begin(&w.backup, EMAIL, Credential::Mp(MP)).err(), Some(ErrorCode::ManifestMismatch));
    w.cleanup(&[&dir]);
}

/// Altering historical recovery_epoch bytes breaks the object hash first,
/// and the checkpoint binding even if the index and manifest are rebuilt.
#[test]
fn provider_alters_historical_recovery_epoch_bytes() {
    let w = world();
    let (out, dev, dir) = recover(&w, "recover");
    let auth = Auth::Device { device_id: dev.device_id(), cred: &out.device_cred };
    let real = head_of(&w, auth);
    let d = snapshot::download(&w.backup, &real.encode(), auth).unwrap();
    let mut entries = d.registry.clone();
    let epoch = entries.iter_mut().find(|e| e.kind == EntryKind::RecoveryEpoch).unwrap();
    epoch.device_name = Some("Attacker Mac".to_string()); // audit-evidence tamper
    let reg_bytes = registry_file::encode(&entries).unwrap();

    // (a) swap the bytes in place: the index hash catches it.
    w.backup.put_object(&w.vault_id, &d.index.registry.key, &reg_bytes, auth).unwrap();
    assert_eq!(begin(&w.backup, EMAIL, Credential::Mp(MP)).err(), Some(ErrorCode::BackupObjectMissing));

    // (b) rebuild index + manifest (attacker-signed): the checkpoint's
    // registry head no longer matches what is served.
    let attacker = SoftwareDevice::generate("Provider", PLATFORM_MACOS);
    let reg_key = vault_helper::backup::index_v1::meta_key("registry", &reg_bytes);
    w.backup.put_object(&w.vault_id, &reg_key, &reg_bytes, auth).unwrap();
    let mut idx = d.index.clone();
    idx.registry = IndexRef::of(reg_key, &reg_bytes);
    let mut m = real.clone();
    m.registry_head = vault_helper::crypto::registry::entry_hash(entries.last().unwrap()).unwrap();
    m.object_index_hash = idx.hash();
    let m = m.sign(&attacker).unwrap();
    w.backup.put_object(&w.vault_id, &ObjectIndex::key(m.generation), &idx.encode(), auth).unwrap();
    let key = vault_helper::backup::index_v1::meta_key("manifest", &m.encode());
    w.backup.put_object(&w.vault_id, &key, &m.encode(), auth).unwrap();
    w.backup.put_object(&w.vault_id, &RegistryCheckpoint::key(m.generation), &d.checkpoint.encode(), auth).unwrap();
    w.backup.force_head(&w.vault_id, &m.encode()).unwrap();
    assert_eq!(begin(&w.backup, EMAIL, Credential::Mp(MP)).err(), Some(ErrorCode::ManifestMismatch));
    w.cleanup(&[&dir]);
}

/// Checkpoint field-by-field: wrong head, wrong generation, wrong VK.
#[test]
fn checkpoint_rejects_wrong_head_generation_or_vk() {
    let w = world();
    let head = head_of(&w, w.auth_mac());
    let good = RegistryCheckpoint::create(&w.vk, &head, 0).unwrap();
    good.verify_binding(&w.vk, &head, &head.registry_head, 0).unwrap();

    let mut wrong_head = head.clone();
    wrong_head.registry_head[0] ^= 1;
    let cp = RegistryCheckpoint::create(&w.vk, &wrong_head, 0).unwrap();
    assert_eq!(cp.verify_binding(&w.vk, &head, &head.registry_head, 0).err(), Some(ErrorCode::ManifestMismatch));

    let mut wrong_gen = head.clone();
    wrong_gen.generation += 1;
    let cp = RegistryCheckpoint::create(&w.vk, &wrong_gen, 0).unwrap();
    assert_eq!(cp.verify_binding(&w.vk, &head, &head.registry_head, 0).err(), Some(ErrorCode::ManifestMismatch));

    // Wrong epoch number, and a MAC under any other VK (e.g. the old VK
    // after a rotation) — both refused.
    assert_eq!(good.verify_binding(&w.vk, &head, &head.registry_head, 1).err(), Some(ErrorCode::ManifestMismatch));
    let other: SecretBytes<32> = random_secret();
    assert_eq!(good.verify(&other).err(), Some(ErrorCode::SignatureInvalid));
    let stale = RegistryCheckpoint::create(&other, &head, 0).unwrap();
    assert_eq!(stale.verify(&w.vk).err(), Some(ErrorCode::SignatureInvalid));
    w.cleanup(&[]);
}

/// A checkpoint made under the pre-rotation VK does not validate the
/// post-rotation state (the checkpoint must be regenerated on rotation).
#[test]
fn checkpoint_under_old_vk_after_rotation_is_refused() {
    let w = world();
    let store = VaultStore::open(&w.a_dir).unwrap();
    let pk = vault_helper::vault::recovery_ops::prove_mp(&store, MP).unwrap();
    let rot = vault_helper::vault::recovery_ops::rotate_recovery_key(store, &w.vk, &pk).unwrap();
    let store = VaultStore::open(&w.a_dir).unwrap();
    let head = snapshot::publish(&w.backup, &store, &w.registry, Some(&w.manifest), &w.mac, &rot.rotation.new_vk, w.auth_mac()).unwrap();
    let served = snapshot::download(&w.backup, &head.encode(), w.auth_mac()).unwrap().checkpoint;
    served.verify(&rot.rotation.new_vk).expect("regenerated under the fresh VK");
    assert!(served.verify(&w.vk).is_err(), "old VK cannot validate the new checkpoint");
    let stale = RegistryCheckpoint::create(&w.vk, &head, 0).unwrap();
    assert!(stale.verify(&rot.rotation.new_vk).is_err());
    w.cleanup(&[]);
}

/// The fresh-device freshness limitation (§11.7) is unchanged: a complete,
/// internally consistent older state still recovers, and the sheet
/// comparison is what exposes it.
#[test]
fn stale_but_valid_complete_state_still_classified_by_the_sheet() {
    let w = world();
    let stale = w.manifest.clone();
    let stale_sheet = SheetCheckpoint::new(w.vault_id, stale.generation, &stale.registry_head);
    let newer = w.a_publishes_edit(&stale, "newer");
    let newer_sheet = SheetCheckpoint::new(w.vault_id, newer.generation, &newer.registry_head);
    w.backup.force_head(&w.vault_id, &stale.encode()).unwrap();

    let s = begin(&w.backup, EMAIL, Credential::Mp(MP)).expect("older complete state still recovers");
    assert_eq!(s.preview().generation, stale.generation);
    assert_eq!(s.compare_sheet(&newer_sheet), SheetComparison::OlderThanSheet);
    assert_eq!(s.compare_sheet(&stale_sheet), SheetComparison::MatchesSheet);
    w.cleanup(&[]);
}

/// Existing devices are unaffected: they still verify epoch proofs with
/// the VK they hold, and still reject rollback and fork.
#[test]
fn existing_device_rollback_and_fork_detection_unchanged() {
    let w = world();
    let old = w.manifest.clone();
    let (out, dev, dir) = recover(&w, "recover");
    let auth = Auth::Device { device_id: dev.device_id(), cred: &out.device_cred };
    let d = snapshot::download(&w.backup, &head_of(&w, auth).encode(), auth).unwrap();
    // Device A kept the pre-recovery VK: proof verification still works.
    let ctx = KnownVk { manifest_hash: old.hash(), vk: *w.vk.expose(), superseded: vec![] };
    let st = verify_chain(&d.registry, &w.vault_id, &ctx).expect("epoch proof verifies for a device that has the VK");
    assert_eq!(st.epoch, 1);
    // …and once A has accepted newer state, the same epoch is a rollback.
    let ctx = KnownVk { manifest_hash: old.hash(), vk: *w.vk.expose(), superseded: vec![old.hash()] };
    assert_eq!(verify_chain(&d.registry, &w.vault_id, &ctx).err(), Some(ErrorCode::ManifestRollback));
    // Fork detection against the locally accepted chain is unchanged.
    let stranger = SoftwareDevice::generate("Stranger", PLATFORM_MACOS);
    let st0 = verify_chain(&w.registry, &w.vault_id, &NoEpochs).unwrap();
    let mut forked = w.registry.clone();
    forked.push(vault_helper::registry::build::enroll(&st0, &w.mac, &stranger).unwrap());
    assert_eq!(
        vault_helper::registry::chain::check_extends(&d.registry, &forked).err(),
        Some(ErrorCode::RegistryFork)
    );
    w.cleanup(&[&dir]);
}
