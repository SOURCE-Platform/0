//! First-device vault creation (spec §5.4, Phase D): VK, vault_id, header,
//! DB, manifest, empty registry (the genesis entry lands with Phase E —
//! no device key exists yet), the MP wrap, and the RK wrap. The caller
//! has already shown the RK to the user in the helper panel (§1.7); the
//! RK and MP bytes never leave the helper.

use std::path::Path;

use crate::crypto::kdf::{self, Argon2Params};
use crate::crypto::secret::{self, SecretBytes};
use crate::crypto::wrap::{self, RecoveryWrapPayload};
use crate::errors::ErrorCode;
use crate::storage::header::{Header, Hex16};
use crate::storage::store::{now_epoch, write_atomic, PASSWORD_WRAP_NAME, RECOVERY_WRAP_NAME};
use crate::storage::VaultStore;
use crate::VAULT_HEADER_NAME;

/// Create every vault file. Returns the header and the VK (callers that
/// only create — `setup_vault` — drop it at once; tests keep it). Any
/// failure removes exactly the files this function may have created.
pub fn create_vault(
    dir: &Path,
    mp: &[u8],
    rk: &SecretBytes<32>,
) -> Result<(Header, SecretBytes<32>), ErrorCode> {
    let result = build(dir, mp, rk);
    if result.is_err() {
        cleanup_partial_vault(dir);
    }
    result
}

fn build(dir: &Path, mp: &[u8], rk: &SecretBytes<32>) -> Result<(Header, SecretBytes<32>), ErrorCode> {
    let vault_id = Hex16::random();
    let vk = secret::random_secret().mlock_best_effort();
    let header = Header::fresh(vault_id);
    let salt = header.kdf.salt_bytes()?;
    let pk = kdf::derive_pk(mp, &salt, header.kdf.params()).map_err(|_| ErrorCode::Internal)?;
    let payload = || RecoveryWrapPayload {
        vk: SecretBytes::new(*vk.expose()),
        wrapped_at: now_epoch(),
        vk_generation: header.vk_generation,
    };
    let mp_wrap = wrap::seal_wrap_mp(&payload(), &pk, &vault_id.0, Argon2Params::V1, &salt)
        .map_err(|_| ErrorCode::Internal)?;
    drop(pk);
    let rk_wrap = wrap::seal_wrap_rk(&payload(), rk, &vault_id.0).map_err(|_| ErrorCode::Internal)?;
    VaultStore::create(dir, header.clone())?;
    // Wraps last: until both exist the directory is not an openable vault.
    write_atomic(
        &dir.join(RECOVERY_WRAP_NAME),
        &serde_json::to_vec_pretty(&rk_wrap).map_err(|_| ErrorCode::Internal)?,
    )?;
    write_atomic(
        &dir.join(PASSWORD_WRAP_NAME),
        &serde_json::to_vec_pretty(&mp_wrap).map_err(|_| ErrorCode::Internal)?,
    )?;
    Ok((header, vk))
}

/// Remove exactly the files vault creation may have created. Never
/// touches helper.sock (the live IPC endpoint shares the directory).
pub fn cleanup_partial_vault(dir: &Path) {
    use crate::storage::{db::DB_NAME, manifest, store};
    for name in [
        VAULT_HEADER_NAME,
        DB_NAME,
        "vault.db-wal",
        "vault.db-shm",
        manifest::MANIFEST_NAME,
        crate::VAULT_REGISTRY_NAME,
        PASSWORD_WRAP_NAME,
        RECOVERY_WRAP_NAME,
    ] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    let _ = std::fs::remove_dir(dir.join(store::WRAPS_DIR));
    let _ = std::fs::remove_dir(dir.join(store::IMPORT_DIR));
}
