//! RF-02…RF-07 (§11.8) and FR-02/FR-03 (§11.7) against FsBackupStore.

mod recovery_fx;

use std::cell::RefCell;

use recovery_fx::*;
use vault_helper::backup::finalize::FinalizeBody;
use vault_helper::backup::fs_recovery::FinalizeFault;
use vault_helper::backup::fs_store::{Auth, RecoveryKind};
use vault_helper::backup::manifest::SignedManifest;
use vault_helper::errors::ErrorCode;
use vault_helper::recovery::creds;
use vault_helper::recovery::sheet::{SheetCheckpoint, SheetComparison};
use vault_helper::recovery::total_loss::{begin, CompletePlan, Credential};
use vault_helper::registry::chain::verify_chain;
use vault_helper::registry::device::{DeviceIdentity, SoftwareDevice, PLATFORM_MACOS};
use vault_helper::storage::VaultStore;
use vault_helper::backup::snapshot;

fn plan<'a>() -> CompletePlan<'a> {
    CompletePlan { new_mp: None, keep_rk: None, fault: None, on_body: None }
}

fn newdev() -> SoftwareDevice {
    SoftwareDevice::generate("Replacement Mac", PLATFORM_MACOS)
}

fn head_gen(w: &World) -> u64 {
    SignedManifest::decode(&w.backup.head_manifest(&w.vault_id, w.auth_mac()).unwrap().unwrap()).unwrap().generation
}

#[test]
fn rf02_stale_expected_head_conflicts_and_mutates_nothing() {
    let w = world();
    let session = begin(&w.backup, EMAIL, Credential::Mp(MP)).unwrap();
    w.a_publishes_edit(&w.manifest, "race"); // the head moves after the session began
    let before = head_gen(&w);
    let dev = newdev();
    let dir = tmp("c");
    assert_eq!(session.complete(&dir, &dev, plan()).err(), Some(ErrorCode::FinalizeConflict));
    assert_eq!(head_gen(&w), before, "head unchanged");
    assert!(!dir.exists(), "local rotated state discarded");
    let cred = [0u8; 32];
    assert!(w.backup.head_manifest(&w.vault_id, Auth::Device { device_id: dev.device_id(), cred: &cred }).is_err());
    w.cleanup(&[]);
}

#[test]
fn rf03_rf04_structurally_invalid_bodies_mutate_nothing() {
    let w = world();
    let before = head_gen(&w);
    let attacker = newdev();
    let tampers: Vec<(&str, Box<dyn Fn(&mut FinalizeBody)>)> = vec![
        ("vk generation not old+1", Box::new(|b| b.new_vk_generation += 1)),
        ("registry head not entry hash", Box::new(|b| b.new_registry_head[0] ^= 1)),
        (
            "RF-04 manifest signed by another key",
            Box::new(move |b| {
                let m = SignedManifest::decode(&b.new_manifest).unwrap();
                let mut resigned = m.sign(&attacker).unwrap();
                resigned.signer_device_id = SignedManifest::decode(&b.new_manifest).unwrap().signer_device_id;
                b.new_manifest = resigned.encode();
            }),
        ),
    ];
    for (name, t) in tampers {
        let dir = tmp("c");
        let r = begin(&w.backup, EMAIL, Credential::Mp(MP)).unwrap().complete(&dir, &newdev(), CompletePlan { on_body: Some(&*t), ..plan() });
        assert!(r.is_err(), "{name}: accepted");
        assert_eq!(head_gen(&w), before, "{name}: head moved");
    }
    w.cleanup(&[]);
}

#[test]
fn rf05_replay_is_idempotent_and_different_body_conflicts() {
    let w = world();
    let captured = RefCell::new(Vec::new());
    let capture = |b: &mut FinalizeBody| *captured.borrow_mut() = b.encode();
    let dir = tmp("c");
    let out = begin(&w.backup, EMAIL, Credential::Mp(MP))
        .unwrap()
        .complete(&dir, &newdev(), CompletePlan { on_body: Some(&capture), ..plan() })
        .unwrap();
    let body = captured.borrow().clone();
    let loc = w.backup.recover_locate(EMAIL).unwrap();
    let pk = vault_helper::crypto::kdf::derive_pk(MP, &loc.kdf_salt, vault_helper::crypto::kdf::Argon2Params::V1).unwrap();
    let c = creds::mp_creds(&pk, &loc.locator_salt_mp).unwrap();
    let replay = w.backup.recovery_finalize(&w.vault_id, &body, RecoveryKind::Mp, c.cred.expose(), None).unwrap();
    assert_eq!(replay.generation, out.generation, "byte-identical replay → stored result");
    assert_eq!(head_gen(&w), out.generation, "no duplicate state");
    let mut other = FinalizeBody::decode(&body).unwrap();
    other.new_device_backup_credential[0] ^= 1;
    assert_eq!(
        w.backup.recovery_finalize(&w.vault_id, &other.encode(), RecoveryKind::Mp, c.cred.expose(), None).err(),
        Some(ErrorCode::FinalizeConflict)
    );
    w.cleanup(&[&dir]);
}

#[test]
fn rf06_crash_mid_finalize_leaves_old_head_then_retry_succeeds() {
    let w = world();
    let before = head_gen(&w);
    let dev = newdev();
    let dir = tmp("c");
    let r = begin(&w.backup, EMAIL, Credential::Mp(MP))
        .unwrap()
        .complete(&dir, &dev, CompletePlan { fault: Some(FinalizeFault::CrashBeforeCommit), ..plan() });
    assert!(r.is_err());
    assert_eq!(head_gen(&w), before, "old head authoritative");
    let out = begin(&w.backup, EMAIL, Credential::Mp(MP)).unwrap().complete(&dir, &dev, plan()).unwrap();
    assert_eq!(out.generation, before + 1);
    w.cleanup(&[&dir]);
}

#[test]
fn rf07_new_device_publishes_recovery_credential_cannot() {
    let w = world();
    let dev = newdev();
    let dir = tmp("c");
    let out = begin(&w.backup, EMAIL, Credential::Mp(MP)).unwrap().complete(&dir, &dev, plan()).unwrap();
    let auth = Auth::Device { device_id: dev.device_id(), cred: &out.device_cred };
    let head = SignedManifest::decode(&w.backup.head_manifest(&w.vault_id, auth).unwrap().unwrap()).unwrap();
    let store = VaultStore::open(&dir).unwrap();
    let registry = snapshot::download(&w.backup, &head.encode(), auth).unwrap().registry;
    snapshot::publish(&w.backup, &store, &registry, Some(&head), &dev, auth).expect("new device publishes");
    // BK-14: the recovery credential can read + finalize only.
    let loc = w.backup.recover_locate(EMAIL).unwrap();
    let pk = vault_helper::crypto::kdf::derive_pk(MP, &loc.kdf_salt, vault_helper::crypto::kdf::Argon2Params::V1).unwrap();
    let c = creds::mp_creds(&pk, &loc.locator_salt_mp).unwrap();
    let rec = Auth::Recovery { kind: RecoveryKind::Mp, cred: c.cred.expose() };
    assert_eq!(w.backup.publish(&w.vault_id, head.generation + 1, &head.encode(), rec).err(), Some(ErrorCode::DeviceNotAuthorized));
    assert!(w.backup.register_recovery(&w.vault_id, RecoveryKind::Mp, &[0; 32], &[0; 32], rec).is_err());
    // BK-13: a revoked device credential is refused before anything runs.
    w.backup.revoke_device(&w.vault_id, w.mac.device_id(), auth).unwrap();
    assert_eq!(w.backup.head_manifest(&w.vault_id, w.auth_mac()).err(), Some(ErrorCode::DeviceNotAuthorized));
    w.cleanup(&[&dir]);
}

/// FR-02: the sheet checkpoint and the comparison the recovery UI offers.
#[test]
fn fr02_sheet_checkpoint_comparison() {
    let w = world();
    let printed = SheetCheckpoint::new(w.vault_id, w.manifest.generation, &w.manifest.registry_head);
    let s = begin(&w.backup, EMAIL, Credential::Mp(MP)).unwrap();
    assert_eq!(s.compare_sheet(&printed), SheetComparison::MatchesSheet);
    assert_eq!(s.preview().registry_head_prefix, printed.registry_head_prefix);
    let other_vault = SheetCheckpoint { vault_id: [9; 16], ..printed.clone() };
    assert_eq!(s.compare_sheet(&other_vault), SheetComparison::DifferentVault);
    let forked = SheetCheckpoint { registry_head_prefix: "00000000".into(), ..printed.clone() };
    assert_eq!(s.compare_sheet(&forked), SheetComparison::ForkEvidence);
    drop(s);
    w.a_publishes_edit(&w.manifest, "later");
    let s = begin(&w.backup, EMAIL, Credential::Mp(MP)).unwrap();
    assert_eq!(s.compare_sheet(&printed), SheetComparison::NewerThanSheet);
    w.cleanup(&[]);
}

/// FR-03: a stale-but-valid manifest served at recovery completes with the
/// freshness display (no claimed detection); a device that later sees
/// newer history surfaces the stale binding from the registry.
#[test]
fn fr03_stale_state_recovers_visibly_and_is_caught_later() {
    let w = world();
    let stale = w.manifest.clone();
    let printed = SheetCheckpoint::new(w.vault_id, stale.generation, &stale.registry_head);
    let newer = w.a_publishes_edit(&stale, "newer");
    let printed_newer = SheetCheckpoint::new(w.vault_id, newer.generation, &newer.registry_head);
    // Malicious provider rolls back and serves the older, validly signed state.
    w.backup.force_head(&w.vault_id, &stale.encode()).unwrap();
    w.backup.set_serve_override(&w.vault_id, Some(&stale.encode())).unwrap();
    let s = begin(&w.backup, EMAIL, Credential::Mp(MP)).unwrap();
    assert_eq!(s.preview().generation, stale.generation, "UI shows the served generation");
    assert_eq!(s.compare_sheet(&printed_newer), SheetComparison::OlderThanSheet, "a newer sheet exposes it");
    assert_eq!(s.compare_sheet(&printed), SheetComparison::MatchesSheet);
    w.backup.set_serve_override(&w.vault_id, None).unwrap();
    let dev = newdev();
    let dir = tmp("c");
    let out = s.complete(&dir, &dev, plan()).expect("recovery completes — no claimed detection");
    // Device A accepted `newer`; it verifies the epoch and sees the stale binding.
    let auth = Auth::Device { device_id: dev.device_id(), cred: &out.device_cred };
    let head = w.backup.head_manifest(&w.vault_id, auth).unwrap().unwrap();
    let registry = snapshot::download(&w.backup, &head, auth).unwrap().registry;
    let ctx = KnownVk { manifest_hash: stale.hash(), vk: *w.vk.expose(), superseded: vec![stale.hash()] };
    assert_eq!(verify_chain(&registry, &w.vault_id, &ctx).err(), Some(ErrorCode::ManifestRollback));
    w.cleanup(&[&dir]);
}
