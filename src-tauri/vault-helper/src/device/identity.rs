//! This device's identity: a pair of Secure Enclave keys plus the public
//! record that names them (§2.7, §2.8).
//!
//! The private keys live in the Enclave, referenced by a per-vault
//! Keychain tag. `device.json` holds only public material — device id,
//! name, platform, the tag, and the two 65-byte public keys — so the
//! helper can answer "who am I" without touching the Enclave, and so a
//! missing SE key is detectable (§2.8: a device whose SE key is gone must
//! re-enroll or recover; that is documented behavior, not corruption).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::se;
use crate::crypto::ecdsa::{self, PUBKEY_LEN, SIGNATURE_LEN};
use crate::crypto::{hex, CryptoError};
use crate::errors::ErrorCode;
use crate::registry::device::{random_uuid, DeviceIdentity, PLATFORM_MACOS};
use crate::storage::store::write_atomic;

pub const DEVICE_FILE_NAME: &str = "device.json";

/// Public identity record (`device.json`, 0600). No secret material.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceFile {
    pub v: u32,
    pub device_id: String,
    pub device_name: String,
    pub platform: u8,
    /// Keychain tag naming this device's two SE keys.
    pub key_tag: String,
    pub sign_pub: String,
    pub agree_pub: String,
}

/// A Secure-Enclave-backed device identity.
pub struct SeDevice {
    id: [u8; 16],
    name: String,
    platform: u8,
    key_tag: String,
    sign_pub: [u8; PUBKEY_LEN],
    agree_pub: [u8; PUBKEY_LEN],
}

fn path(dir: &Path) -> PathBuf {
    dir.join(DEVICE_FILE_NAME)
}

impl SeDevice {
    /// Create this device's identity: fresh id, fresh SE keys for both
    /// roles, record written. Replaces any existing record (vault
    /// creation and re-enrollment both mint a new identity, §4.4 rule 5).
    pub fn create(dir: &Path, name: &str, platform: u8) -> Result<SeDevice, ErrorCode> {
        let id = random_uuid();
        let key_tag = format!("dev.{}", hex::encode(id));
        let sign_pub = se::create_signing_key(&key_tag)?;
        let agree_pub = se::create_agreement_key(&key_tag)?;
        let dev = SeDevice {
            id,
            name: name.to_string(),
            platform,
            key_tag,
            sign_pub,
            agree_pub,
        };
        dev.persist(dir)?;
        Ok(dev)
    }

    /// Load the recorded identity and confirm both SE keys still exist
    /// and still match the record. A mismatch or a missing key is
    /// `DEVICE_NOT_AUTHORIZED` (§2.8).
    pub fn load(dir: &Path) -> Result<SeDevice, ErrorCode> {
        let file = read_file(dir)?;
        let dev = SeDevice::from_file(&file)?;
        if se::signing_public(&dev.key_tag)? != dev.sign_pub
            || se::agreement_public(&dev.key_tag)? != dev.agree_pub
        {
            return Err(ErrorCode::DeviceNotAuthorized);
        }
        Ok(dev)
    }

    /// Load if present, otherwise create. Used at vault setup.
    pub fn load_or_create(dir: &Path, name: &str, platform: u8) -> Result<SeDevice, ErrorCode> {
        if path(dir).exists() {
            SeDevice::load(dir)
        } else {
            SeDevice::create(dir, name, platform)
        }
    }

    /// Public identity only — what `list_devices` and the registry need.
    pub fn from_file(file: &DeviceFile) -> Result<SeDevice, ErrorCode> {
        if file.v != 1 {
            return Err(ErrorCode::FormatTooNew);
        }
        Ok(SeDevice {
            id: hex::decode_array::<16>(&file.device_id).ok_or(ErrorCode::DbCorrupt)?,
            name: file.device_name.clone(),
            platform: file.platform,
            key_tag: file.key_tag.clone(),
            sign_pub: hex::decode_array::<PUBKEY_LEN>(&file.sign_pub).ok_or(ErrorCode::DbCorrupt)?,
            agree_pub: hex::decode_array::<PUBKEY_LEN>(&file.agree_pub)
                .ok_or(ErrorCode::DbCorrupt)?,
        })
    }

    pub fn to_file(&self) -> DeviceFile {
        DeviceFile {
            v: 1,
            device_id: hex::encode(self.id),
            device_name: self.name.clone(),
            platform: self.platform,
            key_tag: self.key_tag.clone(),
            sign_pub: hex::encode(self.sign_pub),
            agree_pub: hex::encode(self.agree_pub),
        }
    }

    pub fn persist(&self, dir: &Path) -> Result<(), ErrorCode> {
        let json = serde_json::to_vec_pretty(&self.to_file()).map_err(|_| ErrorCode::Internal)?;
        write_atomic(&path(dir), &json)
    }

    pub fn key_tag(&self) -> &str {
        &self.key_tag
    }

    pub fn rename(&mut self, name: &str) {
        self.name = name.to_string();
    }

    /// Destroy this device's SE keys and forget the record. Used when a
    /// half-finished setup is rolled back (§5.4: nothing survives a
    /// dismissed Recovery Key window).
    pub fn destroy(self, dir: &Path) {
        se::delete_keys(&self.key_tag);
        let _ = std::fs::remove_file(path(dir));
    }
}

/// Destroy whatever identity a directory records (failed setup, or a
/// test tearing down a synthetic vault). Missing files are not an error.
pub fn wipe(dir: &Path) {
    if let Ok(file) = read_file(dir) {
        se::delete_keys(&file.key_tag);
    }
    let _ = std::fs::remove_file(path(dir));
}

pub fn read_file(dir: &Path) -> Result<DeviceFile, ErrorCode> {
    let bytes = std::fs::read(path(dir)).map_err(|_| ErrorCode::NotFound)?;
    serde_json::from_slice(&bytes).map_err(|_| ErrorCode::DbCorrupt)
}

/// True iff this vault directory already has a device identity.
pub fn exists(dir: &Path) -> bool {
    path(dir).exists()
}

impl DeviceIdentity for SeDevice {
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

    /// SE signing is randomized (§2.7): normalize to low-S before the
    /// bytes enter any hash-chained object, and verify against our own
    /// recorded public key so a wrong-key signature never ships.
    fn sign_prehash(&self, digest: &[u8; 32]) -> Result<[u8; SIGNATURE_LEN], CryptoError> {
        let raw = se::sign_digest(&self.key_tag, digest).map_err(|_| CryptoError::SignatureInvalid)?;
        let sig = ecdsa::normalize_low_s(&raw)?;
        ecdsa::verify_prehash(&self.sign_pub, digest, &sig)?;
        Ok(sig)
    }
}

/// The default name for this Mac, used when a vault is created.
pub fn default_mac_name() -> String {
    let host = std::process::Command::new("scutil")
        .arg("--get")
        .arg("ComputerName")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Mac".to_string());
    host.chars().take(64).collect()
}

pub const DEFAULT_PLATFORM: u8 = PLATFORM_MACOS;
