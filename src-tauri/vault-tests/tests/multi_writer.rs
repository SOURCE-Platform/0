//! Multi-writer protocol with simulated Macs (spec v0.4 §18 Phase F
//! clarification): BK-09/BK-17 (STATE_MOVED → merge → republish), SY-02
//! across devices, EV-01/EV-02 (envelope set; a device adopts a rotation
//! from its envelope), BK-13/EV-04 (a revoked device is cut off at the
//! provider and learns it without deleting anything). Synthetic data only.

mod mfx;

use mfx::*;
use vault_helper::errors::ErrorCode;
use vault_helper::registry::device::DeviceIdentity;

fn pair(tag: &str) -> (Cloud, Mac, Mac) {
    let cloud = Cloud::new(tag);
    let mut a = Mac::new(&format!("{tag}-a"));
    a.setup(&cloud, &format!("synthetic-{tag}@example.test")).unwrap();
    let mut b = Mac::new(&format!("{tag}-b"));
    a.enroll(&cloud, &b);
    b.join(&cloud, a.vid());
    (cloud, a, b)
}

#[test]
fn join_then_concurrent_publishers_merge() {
    let (cloud, mut a, mut b) = pair("mw1");
    a.add("from-a");
    b.add("from-b");
    a.publish(&cloud).unwrap();
    // BK-09: B's transition was built on the old state.
    assert_eq!(b.publish(&cloud), Err(ErrorCode::StateMoved));
    let rep = b.sync(&cloud).unwrap().expect("merged");
    assert_eq!(rep.admitted, 1);
    b.publish(&cloud).unwrap();
    a.sync(&cloud).unwrap();
    assert_eq!(a.titles(), vec!["from-a", "from-b"]);
    assert_eq!(b.titles(), a.titles());
}

/// EV-02: A rotates (RK replacement); B, offline meanwhile, adopts the
/// rotation from its own envelope and still reads everything.
#[test]
fn rotation_adopted_from_envelope() {
    let (cloud, mut a, mut b) = pair("mw2");
    a.add("before-rotation");
    a.publish(&cloud).unwrap();
    b.sync(&cloud).unwrap();
    let vk = a.vk.take().unwrap();
    let store = a.store.take().unwrap();
    let pk = vault_helper::vault::recovery_ops::prove_mp(&store, MP).unwrap();
    let rot = vault_helper::vault::recovery_ops::rotate_recovery_key(store, &vk, &pk, false).unwrap();
    a.store = Some(vault_helper::storage::VaultStore::open(&a.dir).unwrap());
    a.vk = Some(rot.rotation.new_vk);
    assert!(vault_helper::sync::pending::load(&a.store().conn).unwrap().is_some(), "REMOTE_UPDATE_PENDING");
    a.publish(&cloud).unwrap();
    assert!(vault_helper::sync::pending::load(&a.store().conn).unwrap().is_none(), "REMOTE_COMMITTED");
    let rep = b.sync(&cloud).unwrap().unwrap();
    assert!(rep.adopted_vk && rep.adopted_singletons);
    assert_eq!(b.store().header.vk_generation, 2);
    assert_eq!(b.titles(), vec!["before-rotation"]);
    // B keeps writing under the adopted VK.
    b.add("after-rotation");
    b.publish(&cloud).unwrap();
    a.sync(&cloud).unwrap();
    assert_eq!(a.titles(), vec!["after-rotation", "before-rotation"]);
}

/// BK-13 / EV-04 / RC-01M core: A revokes B (both recovery classes
/// re-keyed, VK rotated, B's envelope gone). After the publish B gets the
/// generic 401 and nothing is deleted on B.
#[test]
fn revocation_cuts_off_at_the_provider() {
    let (cloud, mut a, b) = pair("mw3");
    let vk = a.vk.take().unwrap();
    let store = a.store.take().unwrap();
    let new_rk = a.rk_fresh();
    let done = vault_helper::vault::revoke_core::revoke(store, &vk, &a.dev, b.dev.device_id(), MP, &new_rk).unwrap();
    a.store = Some(done.store);
    a.vk = Some(done.vk);
    let p = vault_helper::sync::pending::load(&a.store().conn).unwrap().unwrap();
    assert!(p.security_driven && p.recovery_auth_updates.len() == 2);
    // Until the publish commits, B still authenticates (§11.4).
    assert_eq!(b.read(&cloud, vault_proto::request::Operation::StateGet, None).status, 200);
    a.publish(&cloud).unwrap();
    let r = b.read(&cloud, vault_proto::request::Operation::StateGet, None);
    assert_eq!((r.status, err(&r)), (401, "AUTH_INVALID".into()));
    assert!(b.dir.exists() && b.store().header.vk_generation == 1, "nothing deleted on the revoked device");
}
