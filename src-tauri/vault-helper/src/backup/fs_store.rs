//! `FsBackupStore` (spec §11.1): the directory-backed backup store used
//! ONLY as the Phase D deterministic recovery-rehearsal backend (tests,
//! gate, RC/RF/FR/BK scenarios). It is not the production provider
//! (Phase F) and has no network surface.
//!
//! Layout per vault: `<root>/<vault_id>/objects/...` (content-addressed,
//! immutable) and `<root>/<vault_id>/head.json` — ALL mutable provider
//! state (current manifest, credentials, locators, finalize results). Every
//! mutation writes a new head file and renames it into place, so each
//! provider operation — including `recovery-finalize` — commits atomically
//! or not at all (§11.8 atomicity, RF-06).
//!
//! Request authentication is modelled as presenting the credential
//! (compared by SHA-256). §11.4's HMAC request signing, nonces, and the
//! replay cache are Phase F provider work (documented deviation); the
//! credential *scoping* rules (device vs recovery, revocation) are
//! enforced here because the Phase D scenarios exercise them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::index_v1::{self as index, ObjectIndex};
use super::manifest::SignedManifest;
use crate::crypto::hex;
use crate::errors::ErrorCode;
use crate::registry::file as registry_file;
use crate::storage::store::write_atomic;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryKind {
    Mp,
    Rk,
}

impl RecoveryKind {
    pub fn class(self) -> u8 {
        match self {
            RecoveryKind::Mp => super::finalize::CLASS_MP,
            RecoveryKind::Rk => super::finalize::CLASS_RK,
        }
    }
}

/// Who is making a request.
#[derive(Clone, Copy)]
pub enum Auth<'a> {
    Device { device_id: [u8; 16], cred: &'a [u8; 32] },
    Recovery { kind: RecoveryKind, cred: &'a [u8; 32] },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceCred {
    pub cred_sha: String,
    pub active: bool,
}

/// All mutable provider state for one vault.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Head {
    pub vault_id: String,
    pub email: String,
    pub kdf_salt: String,
    pub locator_salt_mp: String,
    pub locator_salt_rk: String,
    pub generation: u64,
    pub manifest_key: Option<String>,
    /// §4.8 checkpoint object for the current generation.
    pub checkpoint_key: Option<String>,
    pub manifest_hash: String,
    pub registry_head: String,
    pub registry_epoch: u64,
    /// Previous manifest keys (§11.3 retention: 2).
    pub retained: Vec<String>,
    /// locator hex → kind
    pub locators: BTreeMap<String, RecoveryKind>,
    pub recovery_creds: BTreeMap<String, String>,
    pub device_creds: BTreeMap<String, DeviceCred>,
    /// expected_old_generation → sha256(finalize body) (§11.8 replay rule)
    pub finalized: BTreeMap<u64, String>,
}

pub struct FsBackupStore {
    root: PathBuf,
}

/// The checkpoint a publisher/finalizer supplies must describe exactly
/// the manifest and epoch being installed (§4.8).
pub fn check_checkpoint_binding(bytes: &[u8], m: &SignedManifest, epoch: u64) -> Result<(), ErrorCode> {
    let c = super::checkpoint::RegistryCheckpoint::decode(bytes)?;
    let ok = c.vault_id == m.vault_id
        && c.registry_head == m.registry_head
        && c.manifest_core_hash == m.core_hash()
        && c.manifest_generation == m.generation
        && c.vk_generation == m.vk_generation
        && c.epoch == epoch;
    if ok { Ok(()) } else { Err(ErrorCode::ManifestMismatch) }
}

pub fn sha_hex(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

pub fn kind_key(kind: RecoveryKind) -> &'static str {
    match kind {
        RecoveryKind::Mp => "mp",
        RecoveryKind::Rk => "rk",
    }
}

impl FsBackupStore {
    pub fn new(root: &Path) -> FsBackupStore {
        std::fs::create_dir_all(root).expect("backup root");
        FsBackupStore { root: root.to_path_buf() }
    }

    pub(super) fn vault_dir(&self, vault_id: &[u8; 16]) -> PathBuf {
        self.root.join(hex::encode(vault_id))
    }

    pub(super) fn load(&self, vault_id: &[u8; 16]) -> Result<Head, ErrorCode> {
        let bytes = std::fs::read(self.vault_dir(vault_id).join("head.json")).map_err(|_| ErrorCode::BackupUnavailable)?;
        serde_json::from_slice(&bytes).map_err(|_| ErrorCode::BackupUnavailable)
    }

    /// Atomic commit of the whole provider state for one vault.
    pub(super) fn save(&self, vault_id: &[u8; 16], head: &Head) -> Result<(), ErrorCode> {
        let bytes = serde_json::to_vec_pretty(head).map_err(|_| ErrorCode::Internal)?;
        write_atomic(&self.vault_dir(vault_id).join("head.json"), &bytes)
    }

    pub(super) fn authorize(&self, head: &Head, auth: Auth<'_>, allow_recovery: bool) -> Result<(), ErrorCode> {
        match auth {
            Auth::Device { device_id, cred } => {
                let c = head.device_creds.get(&hex::encode(device_id)).ok_or(ErrorCode::DeviceNotAuthorized)?;
                if !c.active || c.cred_sha != sha_hex(cred) {
                    return Err(ErrorCode::DeviceNotAuthorized); // BK-13
                }
            }
            Auth::Recovery { kind, cred } => {
                if !allow_recovery {
                    return Err(ErrorCode::DeviceNotAuthorized); // BK-14
                }
                if head.recovery_creds.get(kind_key(kind)) != Some(&sha_hex(cred)) {
                    return Err(ErrorCode::DeviceNotAuthorized);
                }
            }
        }
        Ok(())
    }

    /// Vault setup at the provider: account (email handle, §12 scenario 3)
    /// plus the first device's credential.
    pub fn create_account(
        &self,
        vault_id: [u8; 16],
        email: &str,
        kdf_salt: [u8; 16],
        locator_salts: ([u8; 16], [u8; 16]),
        device_id: [u8; 16],
        device_cred: &[u8; 32],
    ) -> Result<(), ErrorCode> {
        std::fs::create_dir_all(self.vault_dir(&vault_id).join("objects")).map_err(|_| ErrorCode::Internal)?;
        let mut head = Head {
            vault_id: hex::encode(vault_id),
            email: email.to_string(),
            kdf_salt: hex::encode(kdf_salt),
            locator_salt_mp: hex::encode(locator_salts.0),
            locator_salt_rk: hex::encode(locator_salts.1),
            manifest_hash: hex::encode([0u8; 32]),
            registry_head: hex::encode([0u8; 32]),
            ..Head::default()
        };
        head.device_creds.insert(hex::encode(device_id), DeviceCred { cred_sha: sha_hex(device_cred), active: true });
        self.save(&vault_id, &head)?;
        let accounts = self.root.join("accounts");
        std::fs::create_dir_all(&accounts).map_err(|_| ErrorCode::Internal)?;
        write_atomic(&accounts.join(sha_hex(email.as_bytes())), hex::encode(vault_id).as_bytes())
    }

    /// (Re-)register a recovery locator + credential (§11.4, BK-16): the
    /// previous locator/credential of that kind stops working at once.
    pub fn register_recovery(
        &self,
        vault_id: &[u8; 16],
        kind: RecoveryKind,
        locator: &[u8; 32],
        cred: &[u8; 32],
        auth: Auth<'_>,
    ) -> Result<(), ErrorCode> {
        let mut head = self.load(vault_id)?;
        self.authorize(&head, auth, false)?;
        head.locators.retain(|_, k| *k != kind);
        head.locators.insert(hex::encode(locator), kind);
        head.recovery_creds.insert(kind_key(kind).to_string(), sha_hex(cred));
        self.save(vault_id, &head)
    }

    pub fn update_kdf_salt(&self, vault_id: &[u8; 16], kdf_salt: [u8; 16], auth: Auth<'_>) -> Result<(), ErrorCode> {
        let mut head = self.load(vault_id)?;
        self.authorize(&head, auth, false)?;
        head.kdf_salt = hex::encode(kdf_salt);
        self.save(vault_id, &head)
    }

    pub fn put_object(&self, vault_id: &[u8; 16], key: &str, bytes: &[u8], auth: Auth<'_>) -> Result<(), ErrorCode> {
        let head = self.load(vault_id)?;
        self.authorize(&head, auth, true)?;
        if key.contains("..") || !key.starts_with("objects/") {
            return Err(ErrorCode::InvalidInput);
        }
        let path = self.vault_dir(vault_id).join(key);
        std::fs::create_dir_all(path.parent().expect("parent")).map_err(|_| ErrorCode::Internal)?;
        write_atomic(&path, bytes)
    }

    pub fn get_object(&self, vault_id: &[u8; 16], key: &str, auth: Auth<'_>) -> Result<Vec<u8>, ErrorCode> {
        let head = self.load(vault_id)?;
        self.authorize(&head, auth, true)?;
        self.read_object(vault_id, key)
    }

    pub(super) fn read_object(&self, vault_id: &[u8; 16], key: &str) -> Result<Vec<u8>, ErrorCode> {
        if key.contains("..") {
            return Err(ErrorCode::InvalidInput);
        }
        std::fs::read(self.vault_dir(vault_id).join(key)).map_err(|_| ErrorCode::BackupObjectMissing)
    }

    /// Current head manifest bytes (None before the first publication).
    pub fn head_manifest(&self, vault_id: &[u8; 16], auth: Auth<'_>) -> Result<Option<Vec<u8>>, ErrorCode> {
        let head = self.load(vault_id)?;
        self.authorize(&head, auth, true)?;
        head.manifest_key.as_deref().map(|k| self.read_object(vault_id, k)).transpose()
    }

    /// Every object the index names must exist with the indexed hash.
    pub(super) fn check_index(&self, vault_id: &[u8; 16], m: &SignedManifest) -> Result<ObjectIndex, ErrorCode> {
        let idx_bytes = self.read_object(vault_id, &ObjectIndex::key(m.generation))?;
        let idx = ObjectIndex::decode(&idx_bytes)?;
        if idx.hash() != m.object_index_hash || idx.generation != m.generation {
            return Err(ErrorCode::ManifestMismatch);
        }
        for r in idx.all_refs() {
            index::check(r, &self.read_object(vault_id, &r.key)?)?;
        }
        Ok(idx)
    }

    /// §11.3 step 4: CAS publication by an enrolled device. Structural
    /// checks only (the provider is not a root of trust): generation
    /// +1, chained to the current head, index complete, and the signature
    /// verifies under the signer's key in the uploaded registry.
    pub fn publish(
        &self,
        vault_id: &[u8; 16],
        expected_gen: u64,
        manifest: &[u8],
        checkpoint: &[u8],
        auth: Auth<'_>,
    ) -> Result<(), ErrorCode> {
        let mut head = self.load(vault_id)?;
        self.authorize(&head, auth, false)?;
        let m = SignedManifest::decode(manifest)?;
        if head.generation != expected_gen || m.generation != expected_gen + 1 || m.vault_id != *vault_id {
            return Err(ErrorCode::BackupConflict);
        }
        if hex::encode(m.prev_manifest_hash) != head.manifest_hash {
            return Err(ErrorCode::BackupConflict);
        }
        let idx = self.check_index(vault_id, &m)?;
        let entries = registry_file::decode(&self.read_object(vault_id, &idx.registry.key)?)?;
        let signer = entries
            .iter()
            .find(|e| e.device_id == m.signer_device_id && e.sign_pub.is_some())
            .and_then(|e| e.sign_pub)
            .ok_or(ErrorCode::DeviceNotAuthorized)?;
        if entries.iter().any(|e| e.device_id == m.signer_device_id && e.revoked_at.is_some()) {
            return Err(ErrorCode::DeviceNotAuthorized);
        }
        m.verify(&signer)?;
        let epoch = entries.last().map_or(0, |e| e.epoch);
        // Structural only: the provider holds no VK and cannot verify the
        // checkpoint MAC — it just refuses one that does not describe the
        // state being published (§4.8).
        check_checkpoint_binding(checkpoint, &m, epoch)?;
        let key = index::meta_key("manifest", manifest);
        self.put_raw(vault_id, &key, manifest)?;
        self.put_raw(vault_id, &super::checkpoint::RegistryCheckpoint::key(m.generation), checkpoint)?;
        head.checkpoint_key = Some(super::checkpoint::RegistryCheckpoint::key(m.generation));
        self.advance(&mut head, key, &m, epoch);
        self.save(vault_id, &head)
    }

    pub(super) fn put_raw(&self, vault_id: &[u8; 16], key: &str, bytes: &[u8]) -> Result<(), ErrorCode> {
        let path = self.vault_dir(vault_id).join(key);
        std::fs::create_dir_all(path.parent().expect("parent")).map_err(|_| ErrorCode::Internal)?;
        write_atomic(&path, bytes)
    }

    pub(super) fn advance(&self, head: &mut Head, key: String, m: &SignedManifest, epoch: u64) {
        if let Some(prev) = head.manifest_key.take() {
            head.retained.insert(0, prev);
            head.retained.truncate(2);
        }
        head.manifest_key = Some(key);
        head.generation = m.generation;
        head.manifest_hash = hex::encode(m.hash());
        head.registry_head = hex::encode(m.registry_head);
        head.registry_epoch = epoch;
    }

    /// Retained previous manifests (newest first), for BK-10 / FR-03.
    pub fn retained_manifests(&self, vault_id: &[u8; 16]) -> Result<Vec<Vec<u8>>, ErrorCode> {
        let head = self.load(vault_id)?;
        head.retained.iter().map(|k| self.read_object(vault_id, k)).collect()
    }

    /// §11.4 revocation: deactivate a device credential immediately.
    pub fn revoke_device(&self, vault_id: &[u8; 16], target: [u8; 16], auth: Auth<'_>) -> Result<(), ErrorCode> {
        let mut head = self.load(vault_id)?;
        self.authorize(&head, auth, false)?;
        let c = head.device_creds.get_mut(&hex::encode(target)).ok_or(ErrorCode::NotFound)?;
        c.active = false;
        self.save(vault_id, &head)
    }

    pub fn register_device(&self, vault_id: &[u8; 16], device_id: [u8; 16], cred: &[u8; 32], auth: Auth<'_>) -> Result<(), ErrorCode> {
        let mut head = self.load(vault_id)?;
        self.authorize(&head, auth, false)?;
        head.device_creds.insert(hex::encode(device_id), DeviceCred { cred_sha: sha_hex(cred), active: true });
        self.save(vault_id, &head)
    }

    pub(super) fn account_vault(&self, email: &str) -> Result<[u8; 16], ErrorCode> {
        let b = std::fs::read(self.root.join("accounts").join(sha_hex(email.as_bytes()))).map_err(|_| ErrorCode::NotFound)?;
        hex::decode_array(std::str::from_utf8(&b).map_err(|_| ErrorCode::NotFound)?).ok_or(ErrorCode::NotFound)
    }
}
