//! VK wrap constructions (spec §2.2, §2.5).
//!
//! Two payload shapes, deliberately different (v0.2 finding 1):
//! - `RecoveryWrapPayload` (password.wrap / recovery.wrap): VK only.
//! - `DeviceEnvelopePayload` (devices/<id>.wrap): VK + per-device backup
//!   credential. Envelope *sealing* is HPKE, which is Phase E scope; Phase
//!   B implements the payload codec and the CR-12 split assertions.
//!
//! Payloads are encoded with the canonical TLV codec (§4.2) — a
//! fixed-schema binary form shared with the registry, so no second
//! serialization format exists anywhere in the vault. Wrap *files* are the
//! §2.5 JSON shapes.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use super::hkdf;
use super::kdf::{self, Argon2Params};
use super::secret::{random_nonce, random_salt, SecretBytes};
use super::tlv::{EntryBuilder, EntryReader};
use super::{hex, CryptoError};

// Wrap payload TLV tags (fixed schema, ascending).
const TAG_VK: u8 = 0x01;
const TAG_WRAPPED_AT: u8 = 0x02;
const TAG_VK_GENERATION: u8 = 0x03;
/// v0.3's device backup credential: retired, refused on decode (§2.2).
const TAG_DEVICE_BACKUP_CRED: u8 = 0x04;

pub type VaultId = [u8; 16];

/// §2.2 RecoveryWrapPayload: password.wrap / recovery.wrap contents.
pub struct RecoveryWrapPayload {
    pub vk: SecretBytes<32>,
    pub wrapped_at: u64,
    pub vk_generation: u32,
}

/// §2.2 DeviceEnvelopePayload v2: devices/<id>.wrap contents — the VK
/// only (v0.4 removes the device backup credential; tag 0x04 is refused).
pub struct DeviceEnvelopePayload {
    pub vk: SecretBytes<32>,
    pub wrapped_at: u64,
    pub vk_generation: u32,
}

impl RecoveryWrapPayload {
    pub fn encode(&self) -> Vec<u8> {
        EntryBuilder::new()
            .field_bytes(TAG_VK, self.vk.expose())
            .and_then(|b| b.field_uint(TAG_WRAPPED_AT, self.wrapped_at))
            .and_then(|b| b.field_uint(TAG_VK_GENERATION, self.vk_generation as u64))
            .expect("payload tags are ascending constants")
            .build()
    }

    /// Strict decode. A present 0x04 (backup credential) is a hard error:
    /// recovery wraps never carry one (§2.2, CR-12).
    pub fn parse(bytes: &[u8]) -> Result<Self, CryptoError> {
        let reader = EntryReader::parse(bytes)?;
        if reader.get(TAG_DEVICE_BACKUP_CRED).is_some() {
            return Err(CryptoError::FieldPresence);
        }
        let vk = read_secret32(&reader, TAG_VK)?;
        let wrapped_at = reader
            .get_uint(TAG_WRAPPED_AT)?
            .ok_or(CryptoError::FieldPresence)?;
        let vk_generation = reader
            .get_uint(TAG_VK_GENERATION)?
            .ok_or(CryptoError::FieldPresence)? as u32;
        Ok(RecoveryWrapPayload {
            vk,
            wrapped_at,
            vk_generation,
        })
    }
}

impl DeviceEnvelopePayload {
    pub fn encode(&self) -> Vec<u8> {
        EntryBuilder::new()
            .field_bytes(TAG_VK, self.vk.expose())
            .and_then(|b| b.field_uint(TAG_WRAPPED_AT, self.wrapped_at))
            .and_then(|b| b.field_uint(TAG_VK_GENERATION, self.vk_generation as u64))
            .expect("payload tags are ascending constants")
            .build()
    }

    /// Strict decode. The retired backup credential (0x04) is a hard
    /// error (§2.2 v2, CR-12).
    pub fn parse(bytes: &[u8]) -> Result<Self, CryptoError> {
        let reader = EntryReader::parse(bytes)?;
        if reader.get(TAG_DEVICE_BACKUP_CRED).is_some() {
            return Err(CryptoError::FieldPresence);
        }
        let vk = read_secret32(&reader, TAG_VK)?;
        let wrapped_at = reader
            .get_uint(TAG_WRAPPED_AT)?
            .ok_or(CryptoError::FieldPresence)?;
        let vk_generation = reader
            .get_uint(TAG_VK_GENERATION)?
            .ok_or(CryptoError::FieldPresence)? as u32;
        Ok(DeviceEnvelopePayload {
            vk,
            wrapped_at,
            vk_generation,
        })
    }
}

fn read_secret32(reader: &EntryReader<'_>, tag: u8) -> Result<SecretBytes<32>, CryptoError> {
    let bytes = reader.get(tag).ok_or(CryptoError::FieldPresence)?;
    let array: [u8; 32] = bytes.try_into().map_err(|_| CryptoError::FieldPresence)?;
    Ok(SecretBytes::new(array))
}

// --- Wrap files (§2.5 JSON shapes) ----------------------------------------

// The wrap files' public JSON lives in vault-proto (the provider reads the
// MP wrap's parameter block, §11.3 step 7).
pub use vault_proto::header::{PasswordWrapFile, RecoveryWrapFile, WrapKdf as KdfBlock};

fn wrap_aad(vault_id: &VaultId, kind: &str) -> Vec<u8> {
    let mut aad = b"ov0/wrap".to_vec();
    aad.extend_from_slice(vault_id);
    aad.extend_from_slice(kind.as_bytes());
    aad
}

fn seal(key: &SecretBytes<32>, aad: &[u8], plaintext: &[u8]) -> ([u8; 24], Vec<u8>) {
    let nonce = random_nonce();
    let cipher = XChaCha20Poly1305::new_from_slice(key.expose()).expect("32-byte key");
    let ct = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .expect("AEAD seal of a small payload cannot fail");
    (nonce, ct)
}

fn open(
    key: &SecretBytes<32>,
    aad: &[u8],
    nonce: &[u8; 24],
    ct: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = XChaCha20Poly1305::new_from_slice(key.expose()).expect("32-byte key");
    cipher
        .decrypt(&XNonce::from(*nonce), Payload { msg: ct, aad })
        .map_err(|_| CryptoError::IntegrityFailure)
}

pub fn seal_wrap_mp(
    payload: &RecoveryWrapPayload,
    pk: &SecretBytes<32>,
    vault_id: &VaultId,
    params: Argon2Params,
    kdf_salt: &[u8; 16],
) -> Result<PasswordWrapFile, CryptoError> {
    let wrap_key = hkdf::wrap_key_mp(pk, kdf_salt)?;
    let (nonce, ct) = seal(&wrap_key, &wrap_aad(vault_id, "mp"), &payload.encode());
    Ok(PasswordWrapFile {
        v: 1,
        kind: "mp".to_string(),
        kdf_version: kdf::KDF_VERSION_V1,
        argon2id: KdfBlock {
            m: params.m,
            t: params.t,
            p: params.p,
            salt: hex::encode(kdf_salt),
        },
        nonce: hex::encode(nonce),
        ct: hex::encode(&ct),
    })
}

pub fn open_wrap_mp(
    file: &PasswordWrapFile,
    pk: &SecretBytes<32>,
    vault_id: &VaultId,
) -> Result<RecoveryWrapPayload, CryptoError> {
    let salt: [u8; 16] = hex::decode_array(&file.argon2id.salt).ok_or(CryptoError::KdfParams)?;
    let nonce: [u8; 24] = hex::decode_array(&file.nonce).ok_or(CryptoError::KdfParams)?;
    let ct = hex::decode(&file.ct).ok_or(CryptoError::KdfParams)?;
    let wrap_key = hkdf::wrap_key_mp(pk, &salt)?;
    let plaintext = open(&wrap_key, &wrap_aad(vault_id, "mp"), &nonce, &ct)?;
    RecoveryWrapPayload::parse(&plaintext)
}

pub fn seal_wrap_rk(
    payload: &RecoveryWrapPayload,
    rk: &SecretBytes<32>,
    vault_id: &VaultId,
) -> Result<RecoveryWrapFile, CryptoError> {
    let salt = random_salt();
    let wrap_key = hkdf::wrap_key_rk(rk, &salt)?;
    let (nonce, ct) = seal(&wrap_key, &wrap_aad(vault_id, "rk"), &payload.encode());
    Ok(RecoveryWrapFile {
        v: 1,
        kind: "rk".to_string(),
        salt: hex::encode(salt),
        nonce: hex::encode(nonce),
        ct: hex::encode(&ct),
    })
}

pub fn open_wrap_rk(
    file: &RecoveryWrapFile,
    rk: &SecretBytes<32>,
    vault_id: &VaultId,
) -> Result<RecoveryWrapPayload, CryptoError> {
    let salt: [u8; 16] = hex::decode_array(&file.salt).ok_or(CryptoError::KdfParams)?;
    let nonce: [u8; 24] = hex::decode_array(&file.nonce).ok_or(CryptoError::KdfParams)?;
    let ct = hex::decode(&file.ct).ok_or(CryptoError::KdfParams)?;
    let wrap_key = hkdf::wrap_key_rk(rk, &salt)?;
    let plaintext = open(&wrap_key, &wrap_aad(vault_id, "rk"), &nonce, &ct)?;
    RecoveryWrapPayload::parse(&plaintext)
}
