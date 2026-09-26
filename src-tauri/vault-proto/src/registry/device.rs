//! Device identity as the registry and signed manifests see it (§2.7,
//! §4.3). Production identities are Secure-Enclave keys — Phase E.
//!
//! `SoftwareDevice` is the Phase D **rehearsal** identity: a software
//! P-256 key pair used by the recovery engine's tests and the FsBackupStore
//! scenario rehearsals (§16.7 "simulated devices"). It never backs a real
//! vault, is never persisted, and is replaced by the SE implementation of
//! `DeviceIdentity` in Phase E.

use p256::ecdsa::SigningKey;

use crate::crypto::ecdsa::{self, PUBKEY_LEN, SIGNATURE_LEN};
use crate::crypto::CryptoError;

pub const PLATFORM_MACOS: u8 = 1;
pub const PLATFORM_IOS: u8 = 2;

/// What a registry entry / signed manifest needs from a device.
pub trait DeviceIdentity: Send + Sync {
    fn device_id(&self) -> [u8; 16];
    fn device_name(&self) -> String;
    fn platform(&self) -> u8;
    fn sign_pub(&self) -> [u8; PUBKEY_LEN];
    fn agree_pub(&self) -> [u8; PUBKEY_LEN];
    /// Sign a 32-byte digest (low-S, §2.7).
    fn sign_prehash(&self, digest: &[u8; 32]) -> Result<[u8; SIGNATURE_LEN], CryptoError>;
}

pub struct SoftwareDevice {
    id: [u8; 16],
    name: String,
    platform: u8,
    signing: SigningKey,
    sign_pub: [u8; PUBKEY_LEN],
    agree_pub: [u8; PUBKEY_LEN],
}

impl SoftwareDevice {
    /// Fresh random identity (OsRng scalars; retried until in range).
    pub fn generate(name: &str, platform: u8) -> SoftwareDevice {
        let (signing, sign_pub) = random_keypair();
        let (_, agree_pub) = random_keypair();
        SoftwareDevice {
            id: random_uuid(),
            name: name.to_string(),
            platform,
            signing,
            sign_pub,
            agree_pub,
        }
    }
}

/// Random v4 uuid bytes (device ids, §4.3).
pub fn random_uuid() -> [u8; 16] {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("OS RNG");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    b
}

fn random_keypair() -> (SigningKey, [u8; PUBKEY_LEN]) {
    loop {
        let mut scalar = [0u8; 32];
        getrandom::fill(&mut scalar).expect("OS RNG");
        let field = p256::FieldBytes::from(scalar);
        if let Ok(signing) = SigningKey::from_bytes(&field) {
            let sec1 = signing.verifying_key().to_sec1_bytes();
            let mut pubkey = [0u8; PUBKEY_LEN];
            pubkey.copy_from_slice(&sec1);
            return (signing, pubkey);
        }
    }
}

impl DeviceIdentity for SoftwareDevice {
    fn device_id(&self) -> [u8; 16] {
        self.id
    }
    fn device_name(&self) -> String {
        self.name.clone()
    }
    fn platform(&self) -> u8 {
        self.platform
    }
    fn sign_pub(&self) -> [u8; PUBKEY_LEN] {
        self.sign_pub
    }
    fn agree_pub(&self) -> [u8; PUBKEY_LEN] {
        self.agree_pub
    }
    fn sign_prehash(&self, digest: &[u8; 32]) -> Result<[u8; SIGNATURE_LEN], CryptoError> {
        Ok(ecdsa::dev_sign_prehash(&self.signing, digest))
    }
}
