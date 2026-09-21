//! Device envelopes: `wraps/devices/<device_id>.wrap` (§2.2, §2.5, §5.2).
//!
//! The payload is the §2.2 `DeviceEnvelopePayload` (VK + that device's
//! own backup credential), sealed with HPKE base mode to the device's
//! Secure Enclave agreement key at the §2.12 suite. `info` binds the
//! vault, the device and the enrollment instance (§2.9):
//!
//! ```text
//! info = "ov0/envelope/v1" || vault_id || device_id || enrollment_nonce
//! ```
//!
//! so an envelope cannot be replayed to another device, another vault, or
//! another enrollment of the same device.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::se;
use crate::crypto::hex;
use crate::crypto::wrap::DeviceEnvelopePayload;
use crate::errors::ErrorCode;
use crate::storage::store::{write_atomic, WRAPS_DIR};

pub const ENVELOPE_INFO_PREFIX: &[u8] = b"ov0/envelope/v1";
pub const DEVICES_DIR: &str = "devices";

/// On-disk envelope (§2.5 JSON family; the device-wrap shape is a Phase E
/// addition — the spec fixes the payload and the HPKE parameters, not the
/// file framing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEnvelopeFile {
    pub v: u32,
    pub kind: String,
    pub device_id: String,
    pub enrollment_nonce: String,
    /// HPKE encapsulated key, 65-byte uncompressed P-256 (§2.12).
    pub enc: String,
    pub ct: String,
}

pub fn info(vault_id: &[u8; 16], device_id: &[u8; 16], enrollment_nonce: &[u8; 16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ENVELOPE_INFO_PREFIX.len() + 48);
    out.extend_from_slice(ENVELOPE_INFO_PREFIX);
    out.extend_from_slice(vault_id);
    out.extend_from_slice(device_id);
    out.extend_from_slice(enrollment_nonce);
    out
}

pub fn envelope_path(dir: &Path, device_id: &[u8; 16]) -> PathBuf {
    dir.join(WRAPS_DIR)
        .join(DEVICES_DIR)
        .join(format!("{}.wrap", hex::encode(device_id)))
}

/// Seal a payload to `agree_pub` (the target device's SE agreement key).
pub fn seal_envelope(
    agree_pub: &[u8; 65],
    vault_id: &[u8; 16],
    device_id: &[u8; 16],
    enrollment_nonce: &[u8; 16],
    payload: &DeviceEnvelopePayload,
) -> Result<DeviceEnvelopeFile, ErrorCode> {
    let plaintext = crate::crypto::secret::SecretVec::new(payload.encode());
    let (enc, ct) = se::hpke_seal(agree_pub, &info(vault_id, device_id, enrollment_nonce), &plaintext)?;
    Ok(DeviceEnvelopeFile {
        v: 1,
        kind: "device".to_string(),
        device_id: hex::encode(device_id),
        enrollment_nonce: hex::encode(enrollment_nonce),
        enc: hex::encode(enc),
        ct: hex::encode(ct),
    })
}

/// Open an envelope addressed to *this* device, decapsulating inside the
/// Enclave (§2.12 Path A). The `info` is rebuilt from the file's own
/// fields and the caller's vault id, so a swapped envelope fails the AEAD
/// tag rather than silently decrypting.
pub fn open_envelope(
    key_tag: &str,
    vault_id: &[u8; 16],
    file: &DeviceEnvelopeFile,
) -> Result<DeviceEnvelopePayload, ErrorCode> {
    if file.v != 1 || file.kind != "device" {
        return Err(ErrorCode::FormatTooNew);
    }
    let device_id = hex::decode_array::<16>(&file.device_id).ok_or(ErrorCode::WrapCorrupt)?;
    let nonce = hex::decode_array::<16>(&file.enrollment_nonce).ok_or(ErrorCode::WrapCorrupt)?;
    let enc = hex::decode(&file.enc).ok_or(ErrorCode::WrapCorrupt)?;
    let ct = hex::decode(&file.ct).ok_or(ErrorCode::WrapCorrupt)?;
    let pt = se::hpke_open(key_tag, &info(vault_id, &device_id, &nonce), &enc, &ct)?;
    DeviceEnvelopePayload::parse(&pt).map_err(|_| ErrorCode::WrapCorrupt)
}

pub fn write_envelope(
    dir: &Path,
    device_id: &[u8; 16],
    file: &DeviceEnvelopeFile,
) -> Result<(), ErrorCode> {
    let path = envelope_path(dir, device_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| ErrorCode::Internal)?;
    }
    let json = serde_json::to_vec_pretty(file).map_err(|_| ErrorCode::Internal)?;
    write_atomic(&path, &json)
}

pub fn read_envelope(dir: &Path, device_id: &[u8; 16]) -> Result<DeviceEnvelopeFile, ErrorCode> {
    let bytes = std::fs::read(envelope_path(dir, device_id)).map_err(|_| ErrorCode::NotFound)?;
    serde_json::from_slice(&bytes).map_err(|_| ErrorCode::WrapCorrupt)
}

/// Remove a device's envelope (revocation, §11.4).
pub fn remove_envelope(dir: &Path, device_id: &[u8; 16]) {
    let _ = std::fs::remove_file(envelope_path(dir, device_id));
}

/// Every device id that currently has an envelope on disk.
pub fn list_envelopes(dir: &Path) -> Vec<[u8; 16]> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir.join(WRAPS_DIR).join(DEVICES_DIR)) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(stem) = name.strip_suffix(".wrap") {
            if let Some(id) = hex::decode_array::<16>(stem) {
                out.push(id);
            }
        }
    }
    out.sort();
    out
}
