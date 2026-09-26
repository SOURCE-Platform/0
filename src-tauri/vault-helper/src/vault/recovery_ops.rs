//! Trusted-device recovery flows (spec §12 scenarios 5–7), run by the
//! helper on an UNLOCKED vault. The IPC ops in `vault::rk_ops` call these
//! after the panel collected the secrets; the FsBackupStore rehearsals
//! call them directly.
//!
//! - Scenario 5 (MP forgotten): re-wrap the resident VK under a new MP —
//!   no rotation; `password.wrap` is replaced atomically (no window in
//!   which both the old and the new MP work).
//! - Scenarios 6/7 (RK lost / suspected stolen): new RK + full VK
//!   rotation (v0.3 C12: re-wrapping alone is insufficient). Re-sealing
//!   `password.wrap` under the new VK needs PK, so the current MP is
//!   collected too (the spec leaves that prompt implicit — documented).

use crate::crypto::kdf::{self, Argon2Params};
use crate::crypto::secret::{random_salt, random_secret, SecretBytes};
use crate::crypto::wrap::{self, RecoveryWrapPayload};
use crate::errors::ErrorCode;
use crate::storage::header::KdfBlock;
use crate::storage::rotation::{self, MpWrap, RkWrap, RotationOutcome};
use crate::storage::store::{now_epoch, write_atomic, PASSWORD_WRAP_NAME};
use crate::storage::VaultStore;

/// Scenario 5: new MP for the resident VK. Returns the new KDF salt
/// (callers re-register the MP locator + credential with the provider).
pub fn set_master_password(store: &mut VaultStore, vk: &SecretBytes<32>, new_mp: &[u8]) -> Result<[u8; 16], ErrorCode> {
    let salt = random_salt();
    let pk = kdf::derive_pk(new_mp, &salt, Argon2Params::V1).map_err(|_| ErrorCode::Internal)?;
    let payload = RecoveryWrapPayload {
        vk: SecretBytes::new(*vk.expose()),
        wrapped_at: now_epoch(),
        vk_generation: store.header.vk_generation,
    };
    let file = wrap::seal_wrap_mp(&payload, &pk, &store.header.vault_id.0, Argon2Params::V1, &salt)
        .map_err(|_| ErrorCode::Internal)?;
    // Atomic rename: the old wrap is gone the instant the new one exists.
    write_atomic(
        &store.dir.join(PASSWORD_WRAP_NAME),
        &serde_json::to_vec_pretty(&file).map_err(|_| ErrorCode::Internal)?,
    )?;
    let mut next = store.header.clone();
    next.kdf = KdfBlock::frozen(salt);
    next.auth_salt_mp = crate::storage::header::Hex16::random();
    store.flip(next)?;
    Ok(salt)
}

/// PK for the current MP, proven against the live wrap.
pub fn prove_mp(store: &VaultStore, mp: &[u8]) -> Result<SecretBytes<32>, ErrorCode> {
    let bytes = std::fs::read(store.dir.join(PASSWORD_WRAP_NAME)).map_err(|_| ErrorCode::WrapCorrupt)?;
    let file: wrap::PasswordWrapFile = serde_json::from_slice(&bytes).map_err(|_| ErrorCode::WrapCorrupt)?;
    let (salt, params) = crate::vault::setup::wrap_kdf(&file)?;
    let pk = kdf::derive_pk(mp, &salt, params).map_err(|_| ErrorCode::Internal)?;
    wrap::open_wrap_mp(&file, &pk, &store.header.vault_id.0).map_err(|_| ErrorCode::WrongCredential)?;
    Ok(pk)
}

pub struct RkRotation {
    pub new_rk: SecretBytes<32>,
    pub rotation: RotationOutcome,
}

/// Scenarios 6/7: generate RK′ and rotate VK. Consumes the store (the
/// caller reopens it under the new VK).
pub fn rotate_recovery_key(store: VaultStore, vk: &SecretBytes<32>, pk: &SecretBytes<32>) -> Result<RkRotation, ErrorCode> {
    let new_rk = random_secret();
    let rotation = rotation::rotate(store, vk, MpWrap::Reseal(pk), RkWrap::Seal(&new_rk), None, None)?;
    Ok(RkRotation { new_rk, rotation })
}
