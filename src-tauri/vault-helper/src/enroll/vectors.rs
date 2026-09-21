//! XV-ENROLL (§16.8, Phase E): the deterministic parts of the enrollment
//! and envelope wire contracts, so the Swift side can be checked against
//! the same bytes without a live session.
//!
//! HPKE ciphertexts are randomized and cannot be a fixed known-answer
//! test; what *is* fixed — and what a second implementation gets wrong
//! silently — is the `info` string an envelope is bound to (§2.9), the
//! transcript hash, the SAS derivation, and the ACK digest. Those are
//! the rows below. The RFC 9180 suite itself is pinned by the §2.12
//! vectors committed with the Path A PoC.

use serde_json::{json, Value};

use super::transcript::{self, Binding};
use crate::crypto::hex;
use crate::device::envelope;

// Fixed synthetic inputs; documented, not secrets.
const FP: [u8; 32] = [0xC0; 32];
const SECRET: [u8; 16] = [0xC1; 16];
const NONCE_E: [u8; 16] = [0xC2; 16];
const NONCE_N: [u8; 16] = [0xC3; 16];
const MAC_ID: [u8; 16] = [0xC4; 16];
const NEW_ID: [u8; 16] = [0xC5; 16];
const VAULT_ID: [u8; 16] = [0xC6; 16];
const REGISTRY_HEAD: [u8; 32] = [0xC7; 32];

fn sign_pub() -> [u8; 65] {
    crate::crypto::ecdsa::dev_keypair_from_scalar([0x11; 32]).1
}

fn agree_pub() -> [u8; 65] {
    crate::crypto::ecdsa::dev_keypair_from_scalar([0x33; 32]).1
}

pub fn xv_enroll() -> Value {
    let sign = sign_pub();
    let agree = agree_pub();
    let binding = Binding {
        fp: &FP,
        secret: &SECRET,
        nonce_e: &NONCE_E,
        nonce_n: &NONCE_N,
        mac_device_id: &MAC_ID,
        new_device_id: &NEW_ID,
        sign_pub: &sign,
        agree_pub: &agree,
    };
    let t = transcript::transcript(&binding);
    json!({
        "family": "XV-ENROLL",
        "transcript": {
            "prefix": String::from_utf8_lossy(transcript::TRANSCRIPT_PREFIX),
            "fp": hex::encode(FP),
            "secret": hex::encode(SECRET),
            "secret_base32": transcript::encode_secret(&SECRET),
            "nonce_e": hex::encode(NONCE_E),
            "nonce_n": hex::encode(NONCE_N),
            "mac_device_id": hex::encode(MAC_ID),
            "new_device_id": hex::encode(NEW_ID),
            "sign_pub": hex::encode(sign),
            "agree_pub": hex::encode(agree),
            "transcript_sha256": hex::encode(t),
            "sas": transcript::sas(&t).unwrap(),
        },
        "ack": {
            "prefix": String::from_utf8_lossy(transcript::ACK_PREFIX),
            "registry_head": hex::encode(REGISTRY_HEAD),
            "mac_device_id": hex::encode(MAC_ID),
            "digest_sha256": hex::encode(transcript::ack_digest(&REGISTRY_HEAD, &MAC_ID)),
        },
        "envelope_info": {
            "prefix": String::from_utf8_lossy(envelope::ENVELOPE_INFO_PREFIX),
            "vault_id": hex::encode(VAULT_ID),
            "device_id": hex::encode(NEW_ID),
            "enrollment_nonce": hex::encode(NONCE_E),
            "info": hex::encode(envelope::info(&VAULT_ID, &NEW_ID, &NONCE_E)),
        },
        "suite": {
            "kem": "0x0010 DHKEM(P-256, HKDF-SHA256)",
            "kdf": "0x0001 HKDF-SHA256",
            "aead": "0x0003 ChaCha20-Poly1305",
            "mode": "base (0)",
            "public_key_encoding": "65-byte uncompressed X9.63 (0x04 || X || Y)"
        }
    })
}
