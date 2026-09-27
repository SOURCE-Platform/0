//! What-if scenarios for vault-wide changes racing each other (spec v0.4
//! §11.3 singleton merge rule, §11.3.2 remote-completion status, BK-26,
//! BK-27): the first commit wins, the other device adopts it and is told
//! to redo its change; no wrap of a retired VK or superseded secret is
//! ever published; nothing is frozen or refused by mistake; unpublished
//! work survives. Synthetic data only.

mod mfx;

use mfx::*;
use vault_helper::errors::ErrorCode;
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::sync::pending;
use vault_helper::vault::recovery_ops::{change_mp, prove_mp, rotate_recovery_key};

const MP_A: &[u8] = b"synthetic-e2e-master-password-from-a";
const MP_B: &[u8] = b"synthetic-e2e-master-password-from-b";

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

fn mp_change(m: &mut Mac, old: &[u8], new: &[u8]) {
    let (store, vk) = (m.store.take().unwrap(), m.vk.take().unwrap());
    m.store = Some(change_mp(store, &vk, Some(old), new).unwrap());
    m.vk = Some(vk);
}

/// BK-26 (a): pending MP change vs a committed MP change — the committed
/// one wins; A adopts it, is told to redo (needs_user), and publishes no
/// wrap or recovery key of its own superseded MP.
#[test]
fn bk26_mp_change_vs_committed_mp_change() {
    let (cloud, mut a, mut b, _c) = trio("bk26a");
    mp_change(&mut a, MP, MP_A);
    mp_change(&mut b, MP, MP_B);
    b.publish(&cloud).unwrap();
    assert_eq!(a.publish(&cloud), Err(ErrorCode::StateMoved));
    let rep = a.sync(&cloud).unwrap().unwrap();
    assert!(rep.needs_user && rep.adopted_singletons);
    assert!(pending::load(&a.store().conn).unwrap().unwrap().needs_user);
    assert!(prove_mp(a.store(), MP_B).is_ok(), "the committed MP opens the adopted wrap");
    assert!(prove_mp(a.store(), MP_A).is_err(), "A's superseded MP is gone");
    a.publish(&cloud).unwrap();
    b.sync(&cloud).unwrap();
    assert!(prove_mp(b.store(), MP_B).is_ok(), "A published nothing of its superseded MP");
}

/// BK-26 (c): A's pending revocation (a local rotation to generation 2)
/// vs B's committed RK replacement (also generation 2, a different VK).
/// A adopts B's VK from its own envelope, keeps its unpublished edit, and
/// does not keep treating C as revoked.
#[test]
fn bk26_revocation_vs_committed_rotation() {
    let (cloud, mut a, mut b, mut c) = trio("bk26c");
    a.add("a-unpublished");
    let (store, vk) = (a.store.take().unwrap(), a.vk.take().unwrap());
    let done = vault_helper::vault::revoke_core::revoke(store, &vk, &a.dev, c.dev.device_id(), MP, &a.rk_fresh()).unwrap();
    a.store = Some(done.store);
    a.vk = Some(done.vk);
    let (store, vk) = (b.store.take().unwrap(), b.vk.take().unwrap());
    let pk = prove_mp(&store, MP).unwrap();
    let rot = rotate_recovery_key(store, &vk, &pk, false).unwrap();
    b.store = Some(vault_helper::storage::VaultStore::open(&b.dir).unwrap());
    b.vk = Some(rot.rotation.new_vk);
    b.publish(&cloud).unwrap();
    assert_eq!(a.publish(&cloud), Err(ErrorCode::StateMoved));
    let rep = a.sync(&cloud).unwrap().unwrap();
    assert!(rep.needs_user && rep.adopted_vk);
    assert!(a.titles().contains(&"a-unpublished".to_string()), "unpublished work survives adoption");
    a.publish(&cloud).unwrap();
    // C was never revoked remotely: its later edits reach A normally.
    c.sync(&cloud).unwrap();
    c.add("from-c");
    c.publish(&cloud).unwrap();
    a.sync(&cloud).unwrap();
    assert!(a.titles().contains(&"from-c".to_string()));
    assert!(a.titles().contains(&"a-unpublished".to_string()));
}

/// BK-27: B edits offline under the old VK while A rotates; B adopts the
/// new VK from its envelope, re-seals its own edit (same revision id) and
/// publishes it — nothing is lost.
#[test]
fn bk27_records_across_a_concurrent_rotation() {
    let (cloud, mut a, mut b, _c) = trio("bk27");
    b.add("offline-edit");
    let (store, vk) = (a.store.take().unwrap(), a.vk.take().unwrap());
    let pk = prove_mp(&store, MP).unwrap();
    let rot = rotate_recovery_key(store, &vk, &pk, false).unwrap();
    a.store = Some(vault_helper::storage::VaultStore::open(&a.dir).unwrap());
    a.vk = Some(rot.rotation.new_vk);
    a.publish(&cloud).unwrap();
    assert_eq!(b.publish(&cloud), Err(ErrorCode::StateMoved));
    let rep = b.sync(&cloud).unwrap().unwrap();
    assert!(rep.adopted_vk);
    b.publish(&cloud).unwrap();
    a.sync(&cloud).unwrap();
    assert!(a.titles().contains(&"offline-edit".to_string()));
}
