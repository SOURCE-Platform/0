//! Shared Phase E device fixture: a synthetic enrolling device backed by
//! its own Secure Enclave keys, and the §5.1 steps that drive it through
//! enrollment. Used by the enrollment and revocation suites.
//!
//! The "phone" is a second, independent SE identity on this Mac: it opens
//! the HPKE envelope inside its own Enclave and signs the ACK with a key
//! the enrolling side never sees, so both sides run production code.
//!
//! Not every binary uses every helper; silence per-binary lints.
#![allow(dead_code)]

use serde_json::{json, Value};
use vault_helper::crypto::hex;
use vault_helper::device::identity::SeDevice;
use vault_helper::device::se;
use vault_helper::enroll::transcript;
use vault_helper::registry::device::{DeviceIdentity, PLATFORM_IOS};

use crate::vault_fx::Fx;

pub const FP: [u8; 32] = [0x5A; 32];

/// A synthetic enrolling device, Secure-Enclave-backed like a real one.
pub struct Phone {
    dir: std::path::PathBuf,
    pub dev: SeDevice,
}

impl Phone {
    pub fn new(tag: &str) -> Phone {
        vault_helper::test_support::init_test_namespace();
        let dir = std::env::temp_dir().join(format!("vhphone-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dev = SeDevice::create(&dir, "Synthetic iPhone", PLATFORM_IOS).expect("SE phone");
        Phone { dir, dev }
    }

    pub fn hello(&self, secret: &str, nonce_n: [u8; 16]) -> Value {
        json!({
            "op": "enroll_hello",
            "proto": 2,
            "secret": secret,
            "nonce_n": hex::encode(nonce_n),
            "sign_pub": hex::encode(self.dev.sign_pub()),
            "agree_pub": hex::encode(self.dev.agree_pub()),
            "name": "Synthetic iPhone",
            "platform": PLATFORM_IOS,
        })
    }
}

impl Drop for Phone {
    fn drop(&mut self) {
        se::delete_keys(self.dev.key_tag());
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn begin(fx: &Fx) -> Value {
    let resp = fx.op(json!({"op": "begin_enrollment", "fp": hex::encode(FP)}));
    assert_eq!(resp["ok"], true, "begin_enrollment: {resp}");
    resp
}

/// The SAS the phone computes for itself from the reply (§5.2: the code
/// is never transmitted — both sides derive it).
pub fn phone_sas(phone: &Phone, secret: &str, nonce_n: [u8; 16], reply: &Value) -> String {
    let decoded = decode_secret(secret);
    let t = transcript::transcript(&transcript::Binding {
        fp: &FP,
        secret: &decoded,
        nonce_e: &hex::decode_array::<16>(reply["nonce_e"].as_str().unwrap()).unwrap(),
        nonce_n: &nonce_n,
        mac_device_id: &hex::decode_array::<16>(reply["mac_device_id"].as_str().unwrap()).unwrap(),
        new_device_id: &hex::decode_array::<16>(reply["new_device_id"].as_str().unwrap()).unwrap(),
        sign_pub: &phone.dev.sign_pub(),
        agree_pub: &phone.dev.agree_pub(),
    });
    transcript::sas(&t).unwrap()
}

/// Mirror of the helper's base32 decode, as the phone implements it.
pub fn decode_secret(encoded: &str) -> [u8; 16] {
    let (mut acc, mut bits) = (0u32, 0u32);
    let mut out = Vec::new();
    for ch in encoded.bytes() {
        let idx = transcript::ALPHABET.iter().position(|&c| c == ch).unwrap() as u32;
        acc = (acc << 5) | idx;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    out.truncate(16);
    out.try_into().unwrap()
}


/// Drive one full §5.1 enrollment to a committed registry entry. Returns
/// the new device's id.
pub fn enroll_phone(fx: &Fx, phone: &Phone) -> [u8; 16] {
    let begun = begin(fx);
    let secret = begun["secret"].as_str().unwrap().to_string();
    let nonce_n = [0x42u8; 16];
    let hello = fx.op(phone.hello(&secret, nonce_n));
    assert_eq!(hello["ok"], true, "enroll_hello: {hello}");
    let reply = &hello["reply"];
    assert_eq!(
        hello["sas"].as_str().unwrap(),
        phone_sas(phone, &secret, nonce_n, reply),
        "both sides derive the same SAS"
    );
    let new_id = hex::decode_array::<16>(reply["new_device_id"].as_str().unwrap()).unwrap();
    let mac_id = hex::decode_array::<16>(reply["mac_device_id"].as_str().unwrap()).unwrap();
    let confirmed = fx.op(json!({"op": "enroll_confirm"}));
    assert_eq!(confirmed["ok"], true, "enroll_confirm: {confirmed}");
    let head =
        hex::decode_array::<32>(confirmed["bundle"]["registry_head"].as_str().unwrap()).unwrap();
    let sig = phone
        .dev
        .sign_prehash(&transcript::ack_digest(&head, &mac_id))
        .unwrap();
    let acked = fx.op(json!({"op": "enroll_ack", "signature": hex::encode(sig)}));
    assert_eq!(acked["ok"], true, "enroll_ack: {acked}");
    new_id
}
