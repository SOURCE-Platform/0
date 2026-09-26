//! Total-loss recovery over the provider (spec v0.4 §11.5, §11.8, §12
//! scenarios 3/4): RC-03 (MP), RC-04 (RK, new MP set), RF-07/RF-08 (the
//! epoch device publishes, the old devices are cut off, the new state is
//! sealed only under the fresh VK), FR-01 preview, KD-03 (locate metadata
//! ≠ committed header), wrong credentials. Synthetic data only.

mod mfx;

use mfx::recover::{self, into_mac};
use mfx::*;
use vault_helper::crypto::secret::SecretBytes;
use vault_helper::errors::ErrorCode;
use vault_helper::recovery::complete::Plan;
use vault_helper::recovery::total_loss::Credential;
use vault_proto::request::Operation;

const NEW_MP: &[u8] = b"synthetic-e2e-master-password-0002";

fn world(tag: &str) -> (Cloud, Mac, String) {
    let cloud = Cloud::new(tag);
    let handle = format!("synthetic-{tag}@example.test");
    let mut mac = Mac::new(&format!("{tag}-mac"));
    mac.setup(&cloud, &handle).unwrap();
    mac.add("kept-one");
    mac.add("kept-two");
    mac.publish(&cloud).unwrap();
    (cloud, mac, handle)
}

#[test]
fn rc03_mp_recovery_on_a_fresh_machine() {
    let (cloud, old, handle) = world("rc03");
    let rk = SecretBytes::new(*old.rk.as_ref().unwrap().expose());
    let out = recover::run(&cloud, &handle, Credential::Mp(MP), Plan { new_mp: None, keep_rk: Some(&rk) }, None).expect("recovered");
    // FR-01: the preview described the served state before completion.
    assert_eq!((out.preview.generation, out.preview.item_count), (2, 2));
    assert!(out.completed.new_rk.is_none(), "RK kept");
    let mut new = into_mac(out);
    assert_eq!(new.titles(), vec!["kept-one", "kept-two"]);
    assert_eq!(new.store().header.vk_generation, 2, "RF-08: exactly one rotation");
    // RF-07 / BK-15: the epoch device publishes; the old Mac is revoked.
    new.add("after-recovery");
    new.publish(&cloud).unwrap();
    let r = old.read(&cloud, Operation::StateGet, None);
    assert_eq!((r.status, err(&r)), (401, "AUTH_INVALID".into()));
}

#[test]
fn rc04_rk_recovery_sets_a_new_mp() {
    let (cloud, old, handle) = world("rc04");
    let rk = SecretBytes::new(*old.rk.as_ref().unwrap().expose());
    let out = recover::run(&cloud, &handle, Credential::Rk(&rk), Plan { new_mp: Some(NEW_MP), keep_rk: None }, None).expect("recovered");
    let mac = into_mac(out);
    assert_eq!(mac.titles(), vec!["kept-one", "kept-two"]);
    // The new MP now opens the vault's wrap; the old one does not.
    assert!(vault_helper::vault::recovery_ops::prove_mp(mac.store(), NEW_MP).is_ok());
    assert!(vault_helper::vault::recovery_ops::prove_mp(mac.store(), MP).is_err());
    // A second recovery with the new MP (MP class re-keyed by finalize).
    drop(mac);
    let again = recover::run(&cloud, &handle, Credential::Mp(NEW_MP), Plan { new_mp: None, keep_rk: Some(&rk) }, None);
    recover::discard(again.expect("second recovery with the new MP"));
}

#[test]
fn wrong_credentials_get_no_decryption_result() {
    let (cloud, _old, handle) = world("wrongcred");
    let r = recover::run(&cloud, &handle, Credential::Mp(b"synthetic-not-the-password-000001"), Plan { new_mp: None, keep_rk: None }, None);
    assert_eq!(r.err(), Some(ErrorCode::WrongCredential));
    let bogus = SecretBytes::new([0x42; 32]);
    let r = recover::run(&cloud, &handle, Credential::Rk(&bogus), Plan { new_mp: Some(NEW_MP), keep_rk: None }, None);
    assert_eq!(r.err(), Some(ErrorCode::WrongCredential));
}

/// KD-03: a locate response whose (allowlisted) metadata differs from the
/// authenticated committed header is provider tampering.
#[test]
fn kd03_locate_metadata_mismatch() {
    let (cloud, old, handle) = world("kd03");
    let rk = SecretBytes::new(*old.rk.as_ref().unwrap().expose());
    // The RK class authenticates with the real salt; the provider lies
    // about the MP-class salt, which the RK path does not use.
    let tamper = |v: &mut serde_json::Value| v["auth_salt_mp"] = serde_json::json!("ab".repeat(16));
    let r = recover::run(&cloud, &handle, Credential::Rk(&rk), Plan { new_mp: Some(NEW_MP), keep_rk: None }, Some(&tamper));
    assert_eq!(r.err(), Some(ErrorCode::RecoveryMetadataMismatch));
}
