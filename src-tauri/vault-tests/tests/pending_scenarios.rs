//! Remote-completion bookkeeping under loss and races (spec v0.4 §11.3.2,
//! §11.3 singleton rule; review findings SEC-B2/B3/B4/B6): a lost `200` is
//! recognized from the served state (no false "redo", no stale re-send);
//! an older staging's commit never clears a newer change; a redo after
//! `needs_user` publishes; a pending revocation racing another device's
//! enrollment at the base VK generation is adopted, verified and redone.
//! Synthetic data only.

mod mfx;

use mfx::*;
use vault_helper::errors::ErrorCode;
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::storage::VaultStore;
use vault_helper::sync::publish::{self, Staging};
use vault_helper::sync::{pending, seen};
use vault_helper::vault::recovery_ops::{change_mp, prove_mp, rotate_recovery_key};
use vault_proto::request::Operation;

const MP2: &[u8] = b"synthetic-e2e-master-password-pending-2";

/// Stage and post a publish, but lose the provider's answer.
fn post_losing_the_answer(m: &Mac, cloud: &Cloud) -> Staging {
    let s = seen::load(&m.store().conn).unwrap().unwrap();
    let st = publish::stage_publish(m.store(), &m.registry(), m.vk.as_ref().unwrap(), &m.dev, &s, Vec::new()).unwrap();
    assert_eq!(m.post(cloud, &st).status, 200);
    st
}

fn pending_ops(m: &Mac) -> Option<Vec<pending::PendingOp>> {
    pending::load(&m.store().conn).unwrap().map(|p| p.ops)
}

fn mp_change(m: &mut Mac, old: &[u8], new: &[u8]) {
    let (store, vk) = (m.store.take().unwrap(), m.vk.take().unwrap());
    m.store = Some(change_mp(store, &vk, Some(old), new).unwrap());
    m.vk = Some(vk);
}

fn rk_rotation(m: &mut Mac, mp: &[u8], security_driven: bool) {
    let (store, vk) = (m.store.take().unwrap(), m.vk.take().unwrap());
    let pk = prove_mp(&store, mp).unwrap();
    let rot = rotate_recovery_key(store, &vk, &pk, security_driven).unwrap();
    m.store = Some(VaultStore::open(&m.dir).unwrap());
    m.vk = Some(rot.rotation.new_vk);
    m.rk = Some(rot.new_rk);
}

/// SEC-B4: the setup `create` lands but its `200` is lost; the next sync
/// recognizes it and the vault publishes normally afterwards.
#[test]
fn lost_200_on_create_is_recognized() {
    let cloud = Cloud::new("lost-create");
    let mut a = Mac::new("lost-create-a");
    let rk = vault_helper::crypto::secret::random_secret();
    let (header, vk) = vault_helper::vault::create::create_vault(&a.dir, MP, &rk, &a.dev).unwrap();
    let updates = vault_helper::sync::change::updates_for(&header, Some(MP), Some(&rk)).unwrap();
    let first = vault_helper::vault::setup::stage_create(&a.dir, &a.dev, &vk, "synthetic-lost-create@example.test", updates).unwrap();
    a.store = Some(VaultStore::open(&a.dir).unwrap());
    a.vk = Some(vk);
    a.rk = Some(rk);
    assert_eq!(a.post(&cloud, &first.staging).status, 200); // the answer is lost
    assert_eq!(pending_ops(&a), Some(vec![pending::PendingOp::VaultCreate]));
    let rep = a.sync(&cloud).unwrap().unwrap();
    assert!(!rep.needs_user);
    assert_eq!(pending_ops(&a), None, "recognized as REMOTE_COMMITTED");
    a.add("after-create");
    a.publish(&cloud).expect("no stale re-send, no RECOVERY_AUTH_STALE");
}

/// SEC-B4: an MP change's publish lands but its `200` is lost; sync
/// recognizes it — no false "not applied", nothing re-sent.
#[test]
fn lost_200_on_mp_change_is_recognized() {
    let cloud = Cloud::new("lost-mp");
    let mut a = Mac::new("lost-mp-a");
    a.setup(&cloud, "synthetic-lost-mp@example.test").unwrap();
    mp_change(&mut a, MP, MP2);
    post_losing_the_answer(&a, &cloud);
    let rep = a.sync(&cloud).unwrap().unwrap();
    assert!(!rep.needs_user, "our own change is not someone else's");
    assert_eq!(pending_ops(&a), None);
    assert!(prove_mp(a.store(), MP2).is_ok());
    a.add("after");
    a.publish(&cloud).unwrap();
}

/// SEC-B2: a change made while an older staging is in flight survives
/// that staging's commit, and its own publish cuts the old RK off.
#[test]
fn older_commit_never_clears_a_newer_change() {
    let cloud = Cloud::new("race");
    let mut a = Mac::new("race-a");
    let handle = "synthetic-race@example.test";
    a.setup(&cloud, handle).unwrap();
    mp_change(&mut a, MP, MP2);
    let s = seen::load(&a.store().conn).unwrap().unwrap();
    let st1 = publish::stage_publish(a.store(), &a.registry(), a.vk.as_ref().unwrap(), &a.dev, &s, Vec::new()).unwrap();
    let old_rk = vault_helper::crypto::secret::SecretBytes::new(*a.rk.as_ref().unwrap().expose());
    rk_rotation(&mut a, MP2, true); // the newer change, while st1 is in flight
    let r = a.post(&cloud, &st1);
    a.accept(&st1, &r).unwrap();
    let p = pending::load(&a.store().conn).unwrap().expect("the newer change stays pending");
    assert!(p.security_driven && p.ops.contains(&pending::PendingOp::RkReplacement));
    a.publish(&cloud).expect("the newer change publishes");
    assert_eq!(pending_ops(&a), None);
    let locate = cloud.locate(handle);
    let old = vault_helper::recovery::total_loss::Recovery::begin(ORIGIN, &locate, vault_helper::recovery::total_loss::Credential::Rk(&old_rk), 0).unwrap();
    let req = vault_helper::sync::sign::SignRequest { operation: Operation::StateGet, blob: None, body_sha256: vault_proto::request::body_hash(b""), expected_state: None };
    let none = std::collections::BTreeSet::new();
    let h = old.sign(&req, &vault_helper::sync::sign::SignScope { put_blobs: &none, staged: None }, now()).unwrap();
    let (m, p) = Operation::StateGet.route(&old.locate.vault_id, None).unwrap();
    assert_ne!(cloud.send(m, &p, Some(&h), b"").status, 200, "the old RK is cut off");
}

fn trio(tag: &str) -> (Cloud, Mac, Mac, Mac) {
    let cloud = Cloud::new(tag);
    let mut a = Mac::new(&format!("{tag}-a"));
    a.setup(&cloud, &format!("synthetic-{tag}@example.test")).unwrap();
    let mut b = Mac::new(&format!("{tag}-b"));
    let mut c = Mac::new(&format!("{tag}-c"));
    a.enroll(&cloud, &b);
    a.enroll(&cloud, &c);
    b.join(&cloud, a.vid());
    c.join(&cloud, a.vid());
    (cloud, a, b, c)
}

fn revoke(m: &mut Mac, target: [u8; 16]) {
    let (store, vk) = (m.store.take().unwrap(), m.vk.take().unwrap());
    let done = vault_helper::vault::revoke_core::revoke(store, &vk, &m.dev, target, MP, &m.rk_fresh()).unwrap();
    m.store = Some(done.store);
    m.vk = Some(done.vk);
}

/// SEC-B3 + SEC-B6(a): A's pending revocation of C races B's enrollment
/// of D at the base VK generation. A adopts B's state (verified under the
/// base VK from its own envelope), is told to redo, and the redo cuts C
/// off while D keeps access.
#[test]
fn revocation_racing_an_enrollment_is_adopted_and_redone() {
    let (cloud, mut a, mut b, c) = trio("race-enroll");
    let mut d = Mac::new("race-enroll-d");
    revoke(&mut a, c.dev.device_id());
    b.enroll(&cloud, &d);
    assert_eq!(a.publish(&cloud), Err(ErrorCode::StateMoved));
    let rep = a.sync(&cloud).unwrap().unwrap();
    assert!(rep.needs_user && rep.adopted_singletons);
    revoke(&mut a, c.dev.device_id()); // the redo
    assert!(!pending::load(&a.store().conn).unwrap().unwrap().needs_user, "a redo starts afresh");
    a.publish(&cloud).expect("the redo publishes");
    assert_eq!(pending_ops(&a), None);
    assert_eq!(c.read(&cloud, Operation::StateGet, None).status, 401, "C is cut off");
    d.join(&cloud, a.vid());
    assert_eq!(d.read(&cloud, Operation::StateGet, None).status, 200, "D keeps access");
}

/// RU-03: a staged transition re-sent after an interruption is idempotent
/// — the provider answers the identical body again with the same result.
#[test]
fn resending_the_same_transition_is_idempotent() {
    let cloud = Cloud::new("resend");
    let mut a = Mac::new("resend-a");
    a.setup(&cloud, "synthetic-resend@example.test").unwrap();
    mp_change(&mut a, MP, MP2);
    let st = post_losing_the_answer(&a, &cloud);
    let again = a.post(&cloud, &st);
    assert_eq!(again.status, 200, "{}", err(&again));
    a.accept(&st, &again).unwrap();
    assert_eq!(pending_ops(&a), None);
}

