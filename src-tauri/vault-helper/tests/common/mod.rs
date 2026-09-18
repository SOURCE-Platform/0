//! Shared synthetic fixtures for crypto tests. No real credentials, ever:
//! fixed byte patterns only.
//!
//! Not every binary uses every fixture; silence per-binary dead_code.
#![allow(dead_code)]

use vault_helper::crypto::kdf::{self, Argon2Params};
use vault_helper::crypto::secret::SecretBytes;
use vault_helper::crypto::wrap::RecoveryWrapPayload;

/// Fast Argon2 params for logic tests (8 MiB / t=1 / p=1). The production
/// v1 tuple is exercised separately at full cost (CR-01, kdf unit test,
/// kdf_bench calibration).
pub const FAST: Argon2Params = Argon2Params {
    m: 8 * 1024,
    t: 1,
    p: 1,
};

pub const VAULT_ID: [u8; 16] = [0xA0; 16];
pub const RECORD_ID: [u8; 16] = [0xB0; 16];
pub const KDF_SALT: [u8; 16] = [0xC0; 16];
pub const META_SALT: [u8; 16] = [0xD0; 16];

pub const MP: &[u8] = b"synthetic test master password, not a real credential";

pub fn vk(byte: u8) -> SecretBytes<32> {
    SecretBytes::new([byte; 32])
}

pub fn sample_payload(byte: u8) -> RecoveryWrapPayload {
    RecoveryWrapPayload {
        vk: vk(byte),
        wrapped_at: 1_751_200_000,
        vk_generation: 0,
    }
}

pub fn pk(params: Argon2Params) -> SecretBytes<32> {
    kdf::derive_pk(MP, &KDF_SALT, params).expect("derive pk")
}
