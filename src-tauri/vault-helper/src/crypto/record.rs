//! Record and metadata encryption (spec §2.6). Every record is sealed
//! under a per-record HKDF subkey; moving ciphertext to another record_id,
//! schema, or vk_generation fails AAD (CR-06).

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use super::hkdf;
use super::secret::{random_nonce, SecretBytes, SecretVec};
use super::wrap::VaultId;
use super::CryptoError;

pub type RecordId = [u8; 16];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordCiphertext {
    pub nonce: [u8; 24],
    pub ct: Vec<u8>,
}

/// §2.6: "ov0/record" ‖ vault_id ‖ record_id ‖ u32be(schema) ‖ u32be(gen).
pub fn record_aad(
    vault_id: &VaultId,
    record_id: &RecordId,
    schema_version: u32,
    vk_generation: u32,
) -> Vec<u8> {
    let mut aad = b"ov0/record".to_vec();
    aad.extend_from_slice(vault_id);
    aad.extend_from_slice(record_id);
    aad.extend_from_slice(&schema_version.to_be_bytes());
    aad.extend_from_slice(&vk_generation.to_be_bytes());
    aad
}

/// §2.6: "ov0/meta" ‖ vault_id ‖ record_id ‖ field_tag.
pub fn meta_aad(vault_id: &VaultId, record_id: &RecordId, field_tag: &[u8]) -> Vec<u8> {
    let mut aad = b"ov0/meta".to_vec();
    aad.extend_from_slice(vault_id);
    aad.extend_from_slice(record_id);
    aad.extend_from_slice(field_tag);
    aad
}

fn seal_with(key: &SecretBytes<32>, aad: &[u8], plaintext: &[u8]) -> RecordCiphertext {
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
        .expect("AEAD seal cannot fail for in-memory payloads");
    RecordCiphertext { nonce, ct }
}

fn open_with(
    key: &SecretBytes<32>,
    aad: &[u8],
    sealed: &RecordCiphertext,
) -> Result<SecretVec, CryptoError> {
    let cipher = XChaCha20Poly1305::new_from_slice(key.expose()).expect("32-byte key");
    let plaintext = cipher
        .decrypt(
            &XNonce::from(sealed.nonce),
            Payload {
                msg: &sealed.ct,
                aad,
            },
        )
        .map_err(|_| CryptoError::IntegrityFailure)?;
    Ok(SecretVec::new(plaintext))
}

/// Seal a record's JSON plaintext (§8 schema) under its per-record subkey.
pub fn seal_record(
    vk: &SecretBytes<32>,
    vault_id: &VaultId,
    record_id: &RecordId,
    schema_version: u32,
    vk_generation: u32,
    plaintext: &[u8],
) -> Result<RecordCiphertext, CryptoError> {
    let key = hkdf::record_key(vk, record_id)?;
    Ok(seal_with(
        &key,
        &record_aad(vault_id, record_id, schema_version, vk_generation),
        plaintext,
    ))
}

/// Open a record; AEAD failure maps to RECORD_CORRUPT at the op layer
/// (CR-05: no partial plaintext ever escapes).
pub fn open_record(
    vk: &SecretBytes<32>,
    vault_id: &VaultId,
    record_id: &RecordId,
    schema_version: u32,
    vk_generation: u32,
    sealed: &RecordCiphertext,
) -> Result<SecretVec, CryptoError> {
    let key = hkdf::record_key(vk, record_id)?;
    open_with(
        &key,
        &record_aad(vault_id, record_id, schema_version, vk_generation),
        sealed,
    )
}

/// Seal a metadata field under the vault meta key (§2.6).
pub fn seal_meta(
    vk: &SecretBytes<32>,
    vault_id: &VaultId,
    meta_salt: &[u8; 16],
    record_id: &RecordId,
    field_tag: &[u8],
    plaintext: &[u8],
) -> Result<RecordCiphertext, CryptoError> {
    let key = hkdf::meta_key(vk, meta_salt)?;
    Ok(seal_with(
        &key,
        &meta_aad(vault_id, record_id, field_tag),
        plaintext,
    ))
}

pub fn open_meta(
    vk: &SecretBytes<32>,
    vault_id: &VaultId,
    meta_salt: &[u8; 16],
    record_id: &RecordId,
    field_tag: &[u8],
    sealed: &RecordCiphertext,
) -> Result<SecretVec, CryptoError> {
    let key = hkdf::meta_key(vk, meta_salt)?;
    open_with(&key, &meta_aad(vault_id, record_id, field_tag), sealed)
}
