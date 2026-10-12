//! SEC-I4 (spec v0.5 §2.7; F.2d chunk review SEC-B1 / VER-B2): a Mac whose
//! envelope cannot be opened (a "password every time" Mac, Touch ID
//! unavailable) holds a pending revocation when another device publishes
//! an edit at the base generation. The kept base key opens that state — no
//! envelope, no master password — so the revocation still publishes, and
//! the edit follows re-sealed. Without the kept key the same sync would
//! need the envelope. Synthetic data only.

mod mfx;

use mfx::*;
use vault_helper::errors::ErrorCode;
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::storage::kv;
use vault_helper::sync::base_key::KEY;

#[test]
fn a_pending_revocation_survives_another_devices_publication_without_a_credential() {
    let cloud = Cloud::new("basekey");
    let mut a = Mac::new("basekey-a");
    a.setup(&cloud, "synthetic-basekey@example.test").unwrap();
    let (mut b, mut c) = (Mac::new("basekey-b"), Mac::new("basekey-c"));
    a.enroll(&cloud, &b);
    a.enroll(&cloud, &c);
    b.join(&cloud, a.vid());
    c.join(&cloud, a.vid());
    let base = a.store().header.vk_generation;
    // A revokes C: a pending local rotation on the base generation.
    let (store, vk) = (a.store.take().unwrap(), a.vk.take().unwrap());
    let done = vault_helper::vault::revoke_core::revoke(store, &vk, &a.dev, c.dev.device_id(), MP, &a.rk_fresh()).unwrap();
    a.store = Some(done.store);
    a.vk = Some(done.vk);
    assert_eq!(a.store().header.vk_generation, base + 1);
    // B publishes an edit at the base generation meanwhile.
    b.add("from-b");
    b.publish(&cloud).unwrap();
    assert_eq!(a.publish(&cloud), Err(ErrorCode::StateMoved));
    let no_envelope = |_: &vault_helper::device::envelope::DeviceEnvelopeFile| Err(ErrorCode::DeviceNotAuthorized);
    // Without the kept key the envelope is needed, and refuses.
    let kept: serde_json::Value = kv::get(&a.store().conn, KEY).unwrap().expect("the base key is kept");
    kv::delete(&a.store().conn, KEY).unwrap();
    assert_eq!(a.sync_opening(&cloud, &no_envelope).err(), Some(ErrorCode::DeviceNotAuthorized));
    kv::put(&a.store().conn, KEY, &kept).unwrap();
    // With it: merged without any credential, and the revocation publishes.
    let rep = a.sync_opening(&cloud, &no_envelope).unwrap().expect("merged");
    assert!(!rep.adopted_vk && !rep.needs_user, "A keeps its pending change: {rep:?}");
    a.publish(&cloud).unwrap();
    assert!(kv::get::<serde_json::Value>(&a.store().conn, KEY).unwrap().is_none(), "cleared once published");
    assert_eq!(c.sync(&cloud).err(), Some(ErrorCode::AuthInvalid), "C is cut off at the provider");
    // B's edit, sealed under the base key, comes back re-sealed (BK-27).
    b.sync(&cloud).unwrap();
    assert_eq!(b.store().header.vk_generation, base + 1);
    b.publish(&cloud).unwrap();
    a.sync_opening(&cloud, &no_envelope).unwrap();
    assert!(a.titles().contains(&"from-b".to_string()), "B's edit reached A");
}
