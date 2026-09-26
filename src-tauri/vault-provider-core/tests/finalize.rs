//! §11.8 total-loss finalize at the provider (RF-01…RF-07, BK-14, BK-15).
//! Synthetic data only.

mod pfx;

use pfx::*;
use vault_proto::crypto::recovery_auth::RecoveryClass;
use vault_proto::crypto::secret::random_secret;
use vault_proto::registry::build;
use vault_proto::registry::chain::{verify_chain_with, EpochPolicy};
use vault_proto::registry::device::{DeviceIdentity, SoftwareDevice, PLATFORM_MACOS};
use vault_proto::request::Operation;
use vault_proto::state::{StateTransition, TransitionKind};

/// A finalize draft installing `new_dev`: epoch + one revoke per active
/// device (ascending id, authorized by the new device), VK rotated, only
/// the new device's envelope.
fn finalize_draft(s: &Sim, new_dev: &SoftwareDevice) -> Draft {
    let mut d = s.next_draft();
    let st = verify_chain_with(&d.registry, &s.vault_id, &EpochPolicy::CheckpointAnchored).unwrap();
    let mut prior: Vec<[u8; 16]> = st.devices.iter().filter(|x| !x.revoked).map(|x| x.device_id).collect();
    prior.sort();
    let epoch = build::recovery_epoch(&st, s.vault_id, s.manifest_hash, &d.vk, new_dev).unwrap();
    d.registry.push(epoch);
    for id in prior {
        let st = verify_chain_with(&d.registry, &s.vault_id, &EpochPolicy::CheckpointAnchored).unwrap();
        d.registry.push(build::revoke(&st, new_dev, id).unwrap());
    }
    d.header.vk_generation += 1;
    d.envs = vec![(new_dev.device_id(), env_blob(&new_dev.device_id(), d.header.vk_generation))];
    for r in d.revs.iter_mut() {
        r.vk_generation = d.header.vk_generation;
    }
    d.wrap_mp = mp_wrap(&d.header, "fin");
    d.vk = random_secret();
    d
}

fn post_finalize(s: &Sim, d: &Draft, new_dev: &SoftwareDevice) -> (StateTransition, vault_provider_core::Response) {
    let (t, blobs) = s.build(d, TransitionKind::Finalize, new_dev);
    let key = s.auth_key(RecoveryClass::Mp, &s.cur.as_ref().unwrap().header);
    for b in &blobs {
        let sha: [u8; 32] = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(b)).into();
        let r = s.call(Operation::BlobPut, Some(&sha), b, Who::Rec(&key, RecoveryClass::Mp), None);
        assert_eq!(r.status, 200);
    }
    let body = t.encode().unwrap();
    let r = s.call(Operation::StateCommit, None, &body, Who::Rec(&key, RecoveryClass::Mp), Some(t.expected_state));
    (t, r)
}

/// RF-01 / RF-07 / BK-15: one atomic commit; the epoch device publishes;
/// the old Mac and the recovery key cannot publish.
#[test]
fn rf01_rf07_finalize_then_new_device_publishes() {
    let mut s = Sim::created("rf01");
    let mut d = s.next_draft();
    s.add_record(&mut d, 1);
    assert_eq!(s.publish(d).status, 200);
    let new_dev = SoftwareDevice::generate("Synthetic Mac (recovered)", PLATFORM_MACOS);
    let d = finalize_draft(&s, &new_dev);
    let (t, r) = post_finalize(&s, &d, &new_dev);
    let old_mac = std::mem::replace(&mut s.mac, new_dev);
    s.accept(&r, d, &t);
    assert_eq!(s.generation, 3);
    let g = s.call(Operation::StateGet, None, b"", Who::Dev(&old_mac), None);
    assert_eq!(g.status, 401, "prior device revoked at the provider");
    let d = s.next_draft();
    assert_eq!(s.publish(d).status, 200, "the epoch device publishes");
    // BK-14: a recovery-class key posting a publish is refused.
    let d = s.next_draft();
    let (t, blobs) = s.build(&d, TransitionKind::Publish, &s.mac);
    s.put_blobs(&blobs, &s.mac);
    let key = s.auth_key(RecoveryClass::Mp, &d.header);
    let r = s.commit(&t, Who::Rec(&key, RecoveryClass::Mp));
    assert_eq!((r.status, Sim::error(&r)), (403, "DEVICE_NOT_AUTHORIZED".into()));
}

/// RF-02: a finalize built against a state that has since moved gets
/// STATE_MOVED; the new device is not activated.
#[test]
fn rf02_stale_expected_state() {
    let mut s = Sim::created("rf02");
    let key = s.auth_key(RecoveryClass::Mp, &s.cur.as_ref().unwrap().header);
    let new_dev = SoftwareDevice::generate("Synthetic Mac (recovered)", PLATFORM_MACOS);
    let d = finalize_draft(&s, &new_dev);
    let (t, blobs) = s.build(&d, TransitionKind::Finalize, &new_dev);
    let moved = s.next_draft();
    assert_eq!(s.publish(moved).status, 200);
    for b in &blobs {
        let sha: [u8; 32] = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(b)).into();
        s.call(Operation::BlobPut, Some(&sha), b, Who::Rec(&key, RecoveryClass::Mp), None);
    }
    let r = s.call(Operation::StateCommit, None, &t.encode().unwrap(), Who::Rec(&key, RecoveryClass::Mp), Some(t.expected_state));
    assert_eq!((r.status, Sim::error(&r)), (409, "STATE_MOVED".into()));
    assert_eq!(s.call(Operation::StateGet, None, b"", Who::Dev(&new_dev), None).status, 401, "new device inactive");
}

/// RF-03 / RF-04: structural failures mutate nothing; a manifest not
/// signed by the epoch device is refused.
#[test]
fn rf03_rf04_structural_failures() {
    let s = Sim::created("rf03");
    let new_dev = SoftwareDevice::generate("Synthetic Mac (recovered)", PLATFORM_MACOS);
    let before = s.call(Operation::StateGet, None, b"", Who::Dev(&s.mac), None).body;
    // RF-04: manifest signed by the old Mac instead of the epoch device.
    let d = finalize_draft(&s, &new_dev);
    let (t, blobs) = s.build(&d, TransitionKind::Finalize, &s.mac);
    s.put_blobs(&blobs, &s.mac);
    let key = s.auth_key(RecoveryClass::Mp, &s.cur.as_ref().unwrap().header);
    let r = s.call(Operation::StateCommit, None, &t.encode().unwrap(), Who::Rec(&key, RecoveryClass::Mp), Some(t.expected_state));
    assert_eq!(r.status, 422, "{}", String::from_utf8_lossy(&r.body));
    // RF-03: a revoke missing for a prior device.
    let mut d = finalize_draft(&s, &new_dev);
    d.registry.pop();
    let (_, r) = post_finalize(&s, &d, &new_dev);
    assert_eq!(Sim::error(&r), "REGISTRY_INVALID");
    assert_eq!(s.call(Operation::StateGet, None, b"", Who::Dev(&s.mac), None).body, before, "state unchanged");
}

/// RF-05: byte-identical replay → same result; a different body for the
/// same passed generation → FINALIZE_CONFLICT.
#[test]
fn rf05_finalize_replay_and_conflict() {
    let s = Sim::created("rf05");
    let old_header = s.cur.as_ref().unwrap().header.clone();
    let new_dev = SoftwareDevice::generate("Synthetic Mac (recovered)", PLATFORM_MACOS);
    let d = finalize_draft(&s, &new_dev);
    let (t, r1) = post_finalize(&s, &d, &new_dev);
    assert_eq!(r1.status, 200);
    let key = s.auth_key(RecoveryClass::Mp, &old_header);
    let r2 = s.call(Operation::StateCommit, None, &t.encode().unwrap(), Who::Rec(&key, RecoveryClass::Mp), Some(t.expected_state));
    assert_eq!((r2.status, r2.body.clone()), (200, r1.body.clone()));
    let other = SoftwareDevice::generate("Synthetic Mac (other)", PLATFORM_MACOS);
    let d2 = finalize_draft(&s, &other);
    let (t2, blobs) = s.build(&d2, TransitionKind::Finalize, &other);
    for b in &blobs {
        let sha: [u8; 32] = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(b)).into();
        s.call(Operation::BlobPut, Some(&sha), b, Who::Rec(&key, RecoveryClass::Mp), None);
    }
    let r3 = s.call(Operation::StateCommit, None, &t2.encode().unwrap(), Who::Rec(&key, RecoveryClass::Mp), Some(t2.expected_state));
    assert_eq!(Sim::error(&r3), "FINALIZE_CONFLICT");
}
