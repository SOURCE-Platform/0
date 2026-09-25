//! VK rotation re-seal primitives (spec §2.10). The full rotation
//! transaction (SQLite + manifest flip) is Phase C/D scope; this module is
//! the crypto core CR-08 exercises: every record and both recovery wraps
//! re-sealed under a new VK with `vk_generation` bumped, in memory.

use super::record::{self, RecordCiphertext, RecordId, RevBinding};
use super::secret::SecretBytes;
use super::wrap::{self, PasswordWrapFile, RecoveryWrapFile, RecoveryWrapPayload, VaultId};
use super::CryptoError;

/// A record as the rotation engine sees it: identity + sealed bytes.
pub struct SealedRecord {
    pub record_id: RecordId,
    pub bind: RevBinding,
    pub schema_version: u32,
    pub vk_generation: u32,
    pub ciphertext: RecordCiphertext,
}

/// Re-seal one record from old VK to new VK. AAD binds the generation, so
/// the old VK fails on the new ciphertext and vice versa (CR-08).
pub fn rotate_record(
    record: &SealedRecord,
    old_vk: &SecretBytes<32>,
    new_vk: &SecretBytes<32>,
    vault_id: &VaultId,
    new_generation: u32,
) -> Result<SealedRecord, CryptoError> {
    let plaintext = record::open_record(
        old_vk,
        vault_id,
        &record.record_id,
        &record.bind,
        record.schema_version,
        record.vk_generation,
        &record.ciphertext,
    )?;
    let ciphertext = record::seal_record(
        new_vk,
        vault_id,
        &record.record_id,
        &record.bind,
        record.schema_version,
        new_generation,
        &plaintext,
    )?;
    Ok(SealedRecord {
        record_id: record.record_id,
        bind: record.bind,
        schema_version: record.schema_version,
        vk_generation: new_generation,
        ciphertext,
    })
}

/// Re-wrap a password.wrap under the new VK (MP/PK unchanged, so the same
/// `pk` opens and re-seals; §2.10 rotation step "all wraps rewritten").
pub fn rotate_wrap_mp(
    file: &PasswordWrapFile,
    pk: &SecretBytes<32>,
    vault_id: &VaultId,
    new_vk: &SecretBytes<32>,
    wrapped_at: u64,
    new_generation: u32,
) -> Result<PasswordWrapFile, CryptoError> {
    let payload = wrap::open_wrap_mp(file, pk, vault_id)?;
    let params = super::kdf::Argon2Params {
        m: file.argon2id.m,
        t: file.argon2id.t,
        p: file.argon2id.p,
    };
    let salt: [u8; 16] =
        super::hex::decode_array(&file.argon2id.salt).ok_or(CryptoError::KdfParams)?;
    let next = RecoveryWrapPayload {
        vk: clone_secret(new_vk),
        wrapped_at,
        vk_generation: new_generation,
    };
    let _ = payload; // old payload drops and zeroizes
    wrap::seal_wrap_mp(&next, pk, vault_id, params, &salt)
}

/// Re-wrap a recovery.wrap under the new VK (RK unchanged).
pub fn rotate_wrap_rk(
    file: &RecoveryWrapFile,
    rk: &SecretBytes<32>,
    vault_id: &VaultId,
    new_vk: &SecretBytes<32>,
    wrapped_at: u64,
    new_generation: u32,
) -> Result<RecoveryWrapFile, CryptoError> {
    let payload = wrap::open_wrap_rk(file, rk, vault_id)?;
    let next = RecoveryWrapPayload {
        vk: clone_secret(new_vk),
        wrapped_at,
        vk_generation: new_generation,
    };
    let _ = payload;
    wrap::seal_wrap_rk(&next, rk, vault_id)
}

fn clone_secret(secret: &SecretBytes<32>) -> SecretBytes<32> {
    SecretBytes::new(*secret.expose())
}
