//! AU-07 across devices (§22.4, review VER-I4): a deleted record restored
//! on one Mac comes back as a new record; every device converges on it
//! with no conflict and the tombstone intact. Synthetic data only.

mod mfx;

use mfx::*;

#[test]
fn a_restored_record_converges_without_a_conflict() {
    let cloud = Cloud::new("au07");
    let mut a = Mac::new("au07-a");
    a.setup(&cloud, "synthetic-au07@example.test").unwrap();
    let mut b = Mac::new("au07-b");
    a.enroll(&cloud, &b);
    b.join(&cloud, a.vid());
    let r = a.add("to-restore");
    a.publish(&cloud).unwrap();
    b.sync(&cloud).unwrap();
    {
        let vk = vault_helper::crypto::secret::SecretBytes::new(*b.vk.as_ref().unwrap().expose());
        b.store.as_mut().unwrap().tombstone(&vk, &r).unwrap();
    }
    b.publish(&cloud).unwrap();
    a.sync(&cloud).unwrap();
    assert!(a.titles().is_empty());

    let vk = vault_helper::crypto::secret::SecretBytes::new(*a.vk.as_ref().unwrap().expose());
    let gone = a.store().list_deleted(&vk).unwrap();
    let rev = vault_helper::crypto::hex::decode_array::<32>(gone[0]["revision_id"].as_str().unwrap()).unwrap();
    let new_ref = a.store.as_mut().unwrap().restore_revision(&vk, &r, &rev).unwrap();
    assert_ne!(new_ref, r);
    a.publish(&cloud).unwrap();
    b.sync(&cloud).unwrap();
    assert_eq!(a.titles(), vec!["to-restore"]);
    assert_eq!(b.titles(), vec!["to-restore"]);
    let listed = b.store().list_records(b.vk.as_ref().unwrap()).unwrap();
    assert!(listed.iter().all(|i| i.get("conflicted").is_none()), "no conflict: {listed:?}");
    assert_eq!(b.store().list_deleted(b.vk.as_ref().unwrap()).unwrap().len(), 1, "the tombstone stands");
}
