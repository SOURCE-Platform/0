//! Master-password adoption (spec §2.7, review SEC-I2 of 99760e0; F.2d):
//! a device that cannot open its own envelope in a served state — a Mac
//! whose agreement key was discarded ("password every time"), or one whose
//! Touch ID is unavailable right now — opens the served state's
//! `wrap_mp` with the master password instead. The wrap is listed in the
//! signed index (its bytes are hash-checked here), so a provider cannot
//! substitute one; what it yields is then verified like any served key:
//! the apply checks the whole state under it.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::backup::index::{ObjectIndex, Role};
use crate::crypto::kdf;
use crate::crypto::wrap::{self, DeviceEnvelopePayload, PasswordWrapFile};
use crate::errors::ErrorCode;

/// The served `wrap_mp` blob, exactly as the signed index names it.
pub fn served_wrap(index: &ObjectIndex, blobs: &HashMap<[u8; 32], Vec<u8>>) -> Result<Vec<u8>, ErrorCode> {
    let e = index.find(&Role::WrapMp).ok_or(ErrorCode::ManifestMismatch)?;
    let b = blobs.get(&e.blob).ok_or(ErrorCode::BackupObjectMissing)?;
    if <[u8; 32]>::from(Sha256::digest(b)) != e.blob || b.len() as u64 != e.size {
        return Err(ErrorCode::BackupObjectMissing);
    }
    Ok(b.clone())
}

/// Open a served `wrap_mp` with `mp`; the VK it holds, in the shape the
/// apply takes from an envelope. A wrong password is `WRONG_CREDENTIAL`.
pub fn open_with_mp(wrap_bytes: &[u8], vault_id: &[u8; 16], mp: &[u8]) -> Result<DeviceEnvelopePayload, ErrorCode> {
    let file: PasswordWrapFile = serde_json::from_slice(wrap_bytes).map_err(|_| ErrorCode::WrapCorrupt)?;
    let (salt, params) = crate::vault::setup::wrap_kdf(&file)?;
    let pk = kdf::derive_pk(mp, &salt, params).map_err(|_| ErrorCode::Internal)?;
    match wrap::open_wrap_mp(&file, &pk, vault_id) {
        Ok(p) => Ok(DeviceEnvelopePayload { vk: p.vk, wrapped_at: p.wrapped_at, vk_generation: p.vk_generation }),
        Err(crate::crypto::CryptoError::IntegrityFailure) => Err(ErrorCode::WrongCredential),
        Err(_) => Err(ErrorCode::WrapCorrupt),
    }
}
