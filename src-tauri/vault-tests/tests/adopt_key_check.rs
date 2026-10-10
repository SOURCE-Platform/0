//! MA-05 (spec §2.7, review VER-B2 of F.2d step 1): whatever key a device
//! is handed for a served state — from its envelope or from the master
//! password — the apply verifies the state under it before adopting
//! anything. A wrong key at the right generation is refused with nothing
//! changed; the real one then adopts. Synthetic data only.

mod mfx;

use mfx::*;
use vault_helper::crypto::secret::SecretBytes;
use vault_helper::crypto::wrap::DeviceEnvelopePayload;
use vault_helper::errors::ErrorCode;
use vault_helper::vault::recovery_ops::{prove_mp, rotate_recovery_key};

#[test]
fn a_wrong_key_for_a_served_state_adopts_nothing() {
    let cloud = Cloud::new("keycheck");
    let mut a = Mac::new("keycheck-a");
    a.setup(&cloud, "synthetic-keycheck@example.test").unwrap();
    let mut b = Mac::new("keycheck-b");
    a.enroll(&cloud, &b);
    b.join(&cloud, a.vid());
    a.add("before-rotation");
    a.publish(&cloud).unwrap();
    b.sync(&cloud).unwrap();
    // A rotates its key and publishes generation 2.
    let (store, vk) = (a.store.take().unwrap(), a.vk.take().unwrap());
    let pk = prove_mp(&store, &vk, MP).unwrap();
    let rot = rotate_recovery_key(store, &vk, &pk, false).unwrap();
    a.store = Some(vault_helper::storage::VaultStore::open(&a.dir).unwrap());
    a.vk = Some(rot.rotation.new_vk);
    a.publish(&cloud).unwrap();
    let before = b.store().header.vk_generation;
    let wrong = |_: &vault_helper::device::envelope::DeviceEnvelopeFile| {
        Ok(DeviceEnvelopePayload { vk: SecretBytes::new([0x5A; 32]), wrapped_at: 0, vk_generation: before + 1 })
    };
    assert_eq!(b.sync_opening(&cloud, &wrong).err(), Some(ErrorCode::SignatureInvalid));
    assert_eq!(b.store().header.vk_generation, before, "nothing adopted");
    let rep = b.sync(&cloud).unwrap().unwrap();
    assert!(rep.adopted_vk, "the real key adopts");
    assert!(b.titles().contains(&"before-rotation".to_string()));
}
