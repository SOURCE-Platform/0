//! The §4.7 iPhone envelope catch-up fixture (spec v0.4 §4.7, EV-03
//! simulator half): a synthetic vault with a synthetic phone enrolled
//! (fixed test scalars — never a real device), a VK rotation the phone
//! missed (Mac A revokes Mac B), and everything the phone's steps 1–8
//! read: the served state, the index, the registry blob, its own envelope,
//! and what the phone had accepted before. HPKE sealing is randomized, so
//! this is a fixture, not a deterministic vector: it is written only with
//! `OV0_WRITE_CATCHUP_FIXTURE=1`, and otherwise the committed copy is
//! re-verified here with the Rust verifiers. Synthetic data only.

mod mfx;

use mfx::*;
use serde_json::{json, Value};
use vault_helper::crypto::hex;
use vault_helper::registry::device::{DeviceIdentity, PLATFORM_IOS};
use vault_proto::crypto::ecdsa::{dev_keypair_from_scalar, dev_sign_prehash};
use vault_proto::request::{body_hash, Operation, ProviderRequest, SignerId};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/catchup_v1.json");
const SIGN_SCALAR: [u8; 32] = [0x71; 32];
const AGREE_SCALAR: [u8; 32] = [0x72; 32];
const PHONE_ID: [u8; 16] = [0xE5; 16];

/// A synthetic iPhone with fixed test scalars.
struct FixedPhone;
impl DeviceIdentity for FixedPhone {
    fn device_id(&self) -> [u8; 16] {
        PHONE_ID
    }
    fn device_name(&self) -> String {
        "Synthetic iPhone".into()
    }
    fn platform(&self) -> u8 {
        PLATFORM_IOS
    }
    fn sign_pub(&self) -> [u8; 65] {
        dev_keypair_from_scalar(SIGN_SCALAR).1
    }
    fn agree_pub(&self) -> [u8; 65] {
        dev_keypair_from_scalar(AGREE_SCALAR).1
    }
    fn sign_prehash(&self, digest: &[u8; 32]) -> Result<[u8; 64], vault_proto::crypto::CryptoError> {
        Ok(dev_sign_prehash(&dev_keypair_from_scalar(SIGN_SCALAR).0, digest))
    }
}

fn build() -> Value {
    let cloud = Cloud::new("catchup");
    let mut a = Mac::new("catchup-a");
    a.setup(&cloud, "synthetic-catchup@example.test").unwrap();
    let mut b = Mac::new("catchup-b");
    a.enroll(&cloud, &b);
    b.join(&cloud, a.vid());
    a.enroll_device(&cloud, &FixedPhone);
    a.add("before-rotation");
    a.publish(&cloud).unwrap();
    // What the phone accepted before going offline.
    let before = vault_helper::sync::remote::parse(&a.read(&cloud, Operation::StateGet, None).body).unwrap();
    let reg_before = a.registry();
    // The rotation it misses: A revokes B.
    let (store, vk) = (a.store.take().unwrap(), a.vk.take().unwrap());
    let done = vault_helper::vault::revoke_core::revoke(store, &vk, &a.dev, b.dev.device_id(), MP, &a.rk_fresh()).unwrap();
    a.store = Some(done.store);
    a.vk = Some(done.vk);
    a.publish(&cloud).unwrap();
    let state = a.read(&cloud, Operation::StateGet, None).body;
    let remote = vault_helper::sync::remote::parse(&state).unwrap();
    let index_bytes = a.read(&cloud, Operation::BlobGet, Some(remote.manifest.object_index_hash)).body;
    let index = vault_helper::backup::index::ObjectIndex::decode(&index_bytes).unwrap();
    let mut blobs = serde_json::Map::new();
    blobs.insert(hex::encode(remote.manifest.object_index_hash), json!(hex::encode(&index_bytes)));
    for role in [vault_helper::backup::index::Role::Registry, vault_helper::backup::index::Role::Env { device_id: PHONE_ID }] {
        let e = index.find(&role).unwrap();
        blobs.insert(hex::encode(e.blob), json!(hex::encode(a.read(&cloud, Operation::BlobGet, Some(e.blob)).body)));
    }
    // A state_get request the phone signs, with fixed t and n.
    let key_id = vault_proto::crypto::recovery_auth::key_id(&FixedPhone.sign_pub());
    let req = ProviderRequest::build(ORIGIN, a.vid(), Operation::StateGet, None, SignerId::Device { device_id: PHONE_ID, key_id }, body_hash(b""), None, 1_900_000_000, [0x0D; 16]).unwrap();
    json!({
        "family": "CATCHUP-FIXTURE",
        "note": "synthetic vault; the phone's scalars are fixed test values",
        "origin": ORIGIN,
        "vault_id": hex::encode(a.vid()),
        "phone": {
            "device_id": hex::encode(PHONE_ID),
            "sign_scalar": hex::encode(SIGN_SCALAR),
            "agree_scalar": hex::encode(AGREE_SCALAR),
            "sign_pub": hex::encode(FixedPhone.sign_pub()),
            "agree_pub": hex::encode(FixedPhone.agree_pub()),
        },
        "accepted": {
            "manifest_generation": before.generation,
            "manifest_hash": hex::encode(before.manifest_hash),
            "vk_generation": before.vk_generation,
            "seen_entries": reg_before.entries.len(),
            "seen_head": hex::encode(reg_before.head),
        },
        "state": String::from_utf8(state).unwrap(),
        "blobs": blobs,
        "expect": {
            "generation": remote.generation,
            "vk_generation": remote.vk_generation,
            "manifest_hash": hex::encode(remote.manifest_hash),
            "registry_head": hex::encode(remote.manifest.registry_head),
            "manifest_core_hash": hex::encode(remote.manifest.core_hash()),
            "phone_env_blob": hex::encode(index.find(&vault_helper::backup::index::Role::Env { device_id: PHONE_ID }).unwrap().blob),
        },
        "state_get_request": {
            "t": 1_900_000_000u64,
            "n": hex::encode([0x0D; 16]),
            "key_id": hex::encode(key_id),
            "tlv": hex::encode(req.encode()),
            "prehash": hex::encode(req.prehash()),
        },
    })
}

#[test]
fn catchup_fixture_is_current() {
    if std::env::var("OV0_WRITE_CATCHUP_FIXTURE").as_deref() == Ok("1") {
        let v = build();
        std::fs::create_dir_all(std::path::Path::new(FIXTURE).parent().unwrap()).unwrap();
        std::fs::write(FIXTURE, serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap();
    }
    // The committed fixture still verifies with the Rust verifiers.
    let v: Value = serde_json::from_slice(&std::fs::read(FIXTURE).expect("fixture committed")).unwrap();
    let remote = vault_helper::sync::remote::parse(v["state"].as_str().unwrap().as_bytes()).unwrap();
    assert_eq!(hex::encode(remote.manifest.core_hash()), v["expect"]["manifest_core_hash"]);
    assert!(remote.vk_generation > v["accepted"]["vk_generation"].as_u64().unwrap() as u32, "a rotation the phone missed");
    for (h, bytes) in v["blobs"].as_object().unwrap() {
        assert_eq!(hex::encode(sha(&hex::decode(bytes.as_str().unwrap()).unwrap())), *h, "blob addresses");
    }
    let tlv = hex::decode(v["state_get_request"]["tlv"].as_str().unwrap()).unwrap();
    let req = ProviderRequest::decode(&tlv).unwrap();
    assert_eq!(hex::encode(req.prehash()), v["state_get_request"]["prehash"]);
    let env_blob = hex::decode(v["blobs"][v["expect"]["phone_env_blob"].as_str().unwrap()].as_str().unwrap()).unwrap();
    let env: vault_helper::device::envelope::DeviceEnvelopeFile = serde_json::from_slice(&env_blob).unwrap();
    assert_eq!((env.v, env.device_id.as_str()), (2, hex::encode(PHONE_ID).as_str()));
}
