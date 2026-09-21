//! First-device vault creation (spec §5.4): VK, vault_id, header, DB,
//! manifest, the genesis registry entry, the MP wrap, the RK wrap, and
//! this Mac's own device envelope. The caller has already shown the RK to
//! the user in the helper panel (§1.7); the RK and MP bytes never leave
//! the helper.
//!
//! Phase E addition: the vault is created by a real device identity, so
//! the registry starts at its self-signed genesis entry (§4.4 rule 4) and
//! the creating device gets an envelope carrying the VK and its own
//! backup credential (§2.2, §11.4).

use std::path::Path;

use crate::crypto::kdf::{self, Argon2Params};
use crate::crypto::secret::{self, SecretBytes};
use crate::crypto::wrap::{self, DeviceEnvelopePayload, RecoveryWrapPayload};
use crate::device::creds::DeviceCreds;
use crate::device::envelope;
use crate::registry::build;
use crate::registry::device::DeviceIdentity;
use crate::registry::log;
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
    dev: &dyn DeviceIdentity,
) -> Result<(Header, SecretBytes<32>), ErrorCode> {
    let result = build_vault(dir, mp, rk, dev);
    if result.is_err() {
        cleanup_partial_vault(dir);
    }
    result
}

fn build_vault(
    dir: &Path,
    mp: &[u8],
    rk: &SecretBytes<32>,
    dev: &dyn DeviceIdentity,
) -> Result<(Header, SecretBytes<32>), ErrorCode> {
    let vault_id = Hex16::random();
    let vk = secret::random_secret().mlock_best_effort();
    let mut header = Header::fresh(vault_id);
    // §4.4 rule 4: genesis is the only self-signed entry, and it is what
    // the header's registry head names from the first moment.
    let genesis = build::genesis(dev)?;
    let head = crate::crypto::registry::entry_hash(&genesis).map_err(|_| ErrorCode::Internal)?;
    header.registry_head = crate::storage::header::Hex32(head);
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
    log::write_all(dir, &[genesis])?;
    issue_self_envelope(dir, &header, &vk, dev)?;
    Ok((header, vk))
}

/// The creating device's own envelope: VK + a fresh backup credential
/// (§2.2 DeviceEnvelopePayload), plus the issuer's record of that
/// credential so later rotations can re-seal it (§11.4).
fn issue_self_envelope(
    dir: &Path,
    header: &Header,
    vk: &SecretBytes<32>,
    dev: &dyn DeviceIdentity,
) -> Result<(), ErrorCode> {
    let cred = secret::random_secret();
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|_| ErrorCode::Internal)?;
    let payload = DeviceEnvelopePayload {
        vk: SecretBytes::new(*vk.expose()),
        device_backup_cred: SecretBytes::new(*cred.expose()),
        wrapped_at: now_epoch(),
        vk_generation: header.vk_generation,
    };
    let file = envelope::seal_envelope(
        &dev.agree_pub(),
        &header.vault_id.0,
        &dev.device_id(),
        &nonce,
        &payload,
    )?;
    envelope::write_envelope(dir, &dev.device_id(), &file)?;
    let mut creds = DeviceCreds::default();
    creds.insert(dev.device_id(), &cred);
    creds.save(dir, vk, &header.vault_id.0)
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
    let _ = std::fs::remove_dir_all(dir.join(store::WRAPS_DIR).join(crate::device::envelope::DEVICES_DIR));
    let _ = std::fs::remove_file(dir.join(crate::device::identity::DEVICE_FILE_NAME));
    let _ = std::fs::remove_dir(dir.join(store::WRAPS_DIR));
    let _ = std::fs::remove_dir(dir.join(store::IMPORT_DIR));
}
