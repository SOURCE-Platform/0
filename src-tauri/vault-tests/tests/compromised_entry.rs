//! §22.12 COMPROMISED entry (CX-01…CX-03, CX-05): only fork evidence
//! signed by a device active in the provider-confirmed registry counts.
//! A foreign key or a revoked device's key can never freeze the vault; a
//! device this Mac is still revoking still counts (§11.3 rule 2).
//! Synthetic data only.

mod mfx;

use mfx::*;
use sha2::{Digest, Sha256};
use vault_helper::errors::ErrorCode;
use vault_helper::registry::device::{DeviceIdentity, SoftwareDevice, PLATFORM_MACOS};
use vault_helper::sync::remote::{self, RemoteState};
use vault_helper::sync::fetch;
use vault_proto::request::Operation;

fn pair(tag: &str) -> (Cloud, Mac, Mac) {
    let cloud = Cloud::new(tag);
    let mut a = Mac::new(&format!("{tag}-a"));
    a.setup(&cloud, &format!("synthetic-{tag}@example.test")).unwrap();
    let mut b = Mac::new(&format!("{tag}-b"));
    a.enroll(&cloud, &b);
    b.join(&cloud, a.vid());
    (cloud, a, b)
}

/// The provider's current state with a different manifest at the same
/// generation, signed by `signer`: a same-generation fork.
fn fork(reader: &Mac, cloud: &Cloud, signer: &dyn DeviceIdentity) -> RemoteState {
    let mut r = remote::parse(&reader.read(cloud, Operation::StateGet, None).body).unwrap();
    let mut m = r.manifest.clone();
    m.created_at += 1;
    let m = m.sign(signer).unwrap();
    r.manifest_bytes = m.encode();
    r.manifest_hash = Sha256::digest(&r.manifest_bytes).into();
    r.manifest = m;
    r
}

fn not_compromised(m: &Mac) {
    assert!(vault_helper::storage::compromised::load(&m.store().conn).unwrap().is_none());
}

/// CX-01: a key the vault never installed.
#[test]
fn cx01_foreign_signature_is_refused_not_compromised() {
    let (cloud, a, _b) = pair("cx01");
    let evil = SoftwareDevice::generate("Someone else's device", PLATFORM_MACOS);
    assert_eq!(fetch::offer(a.store(), &fork(&a, &cloud, &evil)), Err(ErrorCode::SignatureInvalid));
    not_compromised(&a);
}

/// CX-02: a revoked device's (still valid) key, after the revocation
/// committed at the provider.
#[test]
fn cx02_revoked_device_cannot_freeze_the_vault() {
    let (cloud, mut a, b) = pair("cx02");
    let (store, vk) = (a.store.take().unwrap(), a.vk.take().unwrap());
    let done = vault_helper::vault::revoke_core::revoke(store, &vk, &a.dev, b.dev.device_id(), MP, &a.rk_fresh()).unwrap();
    a.store = Some(done.store);
    a.vk = Some(done.vk);
    a.publish(&cloud).unwrap();
    assert_eq!(fetch::offer(a.store(), &fork(&a, &cloud, &b.dev)), Err(ErrorCode::SignatureInvalid));
    not_compromised(&a);
}

/// CX-03: two validly signed manifests by active devices are fork
/// evidence.
#[test]
fn cx03_fork_signed_by_an_active_device_is_evidence() {
    let (cloud, a, b) = pair("cx03");
    assert_eq!(fetch::offer(a.store(), &fork(&a, &cloud, &b.dev)), Err(ErrorCode::RegistryFork));
}

/// CX-05: while this Mac's revocation of B is still pending, B is still
/// active in the provider-confirmed registry, so B's fork is evidence.
#[test]
fn cx05_racing_revocation_is_fork_evidence() {
    let (cloud, mut a, b) = pair("cx05");
    let (store, vk) = (a.store.take().unwrap(), a.vk.take().unwrap());
    let done = vault_helper::vault::revoke_core::revoke(store, &vk, &a.dev, b.dev.device_id(), MP, &a.rk_fresh()).unwrap();
    a.store = Some(done.store);
    a.vk = Some(done.vk);
    // Not published: the provider still lists B as active.
    assert_eq!(fetch::offer(a.store(), &fork(&a, &cloud, &b.dev)), Err(ErrorCode::RegistryFork));
}

/// CX-05, registry-entry form (§11.3 rule 2, review VER-I3): while this
/// Mac's revoke(B) is pending, B commits an entry at the same seq (it
/// enrolls a device of its own). That is fork evidence — never adopted,
/// so the device B enrolled never becomes active here.
#[test]
fn cx05_racing_revocation_entry_is_never_adopted() {
    let (cloud, mut a, mut b) = pair("cx05r");
    let (store, vk) = (a.store.take().unwrap(), a.vk.take().unwrap());
    let done = vault_helper::vault::revoke_core::revoke(store, &vk, &a.dev, b.dev.device_id(), MP, &a.rk_fresh()).unwrap();
    a.store = Some(done.store);
    a.vk = Some(done.vk);
    let thief = SoftwareDevice::generate("Thief's device", PLATFORM_MACOS);
    b.enroll_device(&cloud, &thief);
    assert_eq!(a.sync(&cloud).err(), Some(ErrorCode::RegistryFork));
    assert!(a.registry().active_device(&thief.device_id()).is_none(), "nothing adopted");
    assert!(a.registry().active_device(&b.dev.device_id()).is_none(), "the revocation stands locally");
}
