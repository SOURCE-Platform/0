//! FsBackupStore's recovery endpoints (spec §12 scenario 3, §11.8):
//! locate by email, fetch the bundle by locator, and the atomic
//! `recovery-finalize` transaction with the provider's structural checks.
//! Rehearsal backend only (see `fs_store`).

use super::finalize::FinalizeBody;
use super::fs_store::{sha_hex, Auth, FsBackupStore, RecoveryKind};
use super::index_v1 as index;
use super::manifest::SignedManifest;
use crate::crypto::hex;
use crate::crypto::registry::{self, EntryKind, RegistryEntry};
use crate::errors::ErrorCode;
use crate::registry::file as registry_file;

/// `POST /v1/recover/locate {email}` (§12 scenario 3 step 2).
pub struct LocateInfo {
    pub vault_id: [u8; 16],
    pub kdf_salt: [u8; 16],
    pub locator_salt_mp: [u8; 16],
    pub locator_salt_rk: [u8; 16],
}

pub struct Bundle {
    pub vault_id: [u8; 16],
    pub manifest: Vec<u8>,
}

pub struct FinalizeResult {
    pub generation: u64,
    pub manifest_hash: [u8; 32],
}

/// Test-only fault injection for RF-06.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FinalizeFault {
    CrashBeforeCommit,
}

fn salt(h: &str) -> Result<[u8; 16], ErrorCode> {
    hex::decode_array(h).ok_or(ErrorCode::BackupUnavailable)
}

impl FsBackupStore {
    pub fn recover_locate(&self, email: &str) -> Result<LocateInfo, ErrorCode> {
        let vault_id = self.account_vault(email)?;
        let head = self.load(&vault_id)?;
        Ok(LocateInfo {
            vault_id,
            kdf_salt: salt(&head.kdf_salt)?,
            locator_salt_mp: salt(&head.locator_salt_mp)?,
            locator_salt_rk: salt(&head.locator_salt_rk)?,
        })
    }

    /// `GET /v1/recover/bundle {locator}`. An unknown locator is 404 —
    /// never a decryption result (wrong MP/RK ⇒ wrong locator).
    pub fn recover_bundle(
        &self,
        vault_id: &[u8; 16],
        locator: &[u8; 32],
        kind: RecoveryKind,
        cred: &[u8; 32],
    ) -> Result<Bundle, ErrorCode> {
        let head = self.load(vault_id)?;
        if head.locators.get(&hex::encode(locator)) != Some(&kind) {
            return Err(ErrorCode::NotFound);
        }
        self.authorize(&head, Auth::Recovery { kind, cred }, true)?;
        let key = match std::fs::read_to_string(self.vault_dir(vault_id).join("serve_override")) {
            Ok(k) => k, // test hook: a malicious provider serving older state (FR-03)
            Err(_) => head.manifest_key.clone().ok_or(ErrorCode::BackupUnavailable)?,
        };
        Ok(Bundle { vault_id: *vault_id, manifest: self.read_object(vault_id, &key)? })
    }

    /// Test hook (FR-03): make `recover_bundle` serve an older, still
    /// validly signed manifest. `None` restores honest behavior.
    pub fn set_serve_override(&self, vault_id: &[u8; 16], manifest: Option<&[u8]>) -> Result<(), ErrorCode> {
        let path = self.vault_dir(vault_id).join("serve_override");
        match manifest {
            Some(m) => std::fs::write(path, index::meta_key("manifest", m)).map_err(|_| ErrorCode::Internal),
            None => {
                let _ = std::fs::remove_file(path);
                Ok(())
            }
        }
    }

    /// Test hook (FR-03): a malicious provider rolls its head back to an
    /// older, still validly signed manifest it retained.
    pub fn force_head(&self, vault_id: &[u8; 16], manifest: &[u8]) -> Result<(), ErrorCode> {
        let mut head = self.load(vault_id)?;
        let m = SignedManifest::decode(manifest)?;
        let key = index::meta_key("manifest", manifest);
        head.manifest_key = Some(key);
        head.generation = m.generation;
        head.manifest_hash = hex::encode(m.hash());
        head.registry_head = hex::encode(m.registry_head);
        self.save(vault_id, &head)
    }

    /// §11.8 — the only mutation a recovery credential may authorize.
    pub fn recovery_finalize(
        &self,
        vault_id: &[u8; 16],
        body_bytes: &[u8],
        kind: RecoveryKind,
        cred: &[u8; 32],
        fault: Option<FinalizeFault>,
    ) -> Result<FinalizeResult, ErrorCode> {
        let mut head = self.load(vault_id)?;
        // 1–2: authenticate; this endpoint only, recovery class only.
        self.authorize(&head, Auth::Recovery { kind, cred }, true)?;
        let body = FinalizeBody::decode(body_bytes)?;
        if body.recovery_credential_class != kind.class() || body.vault_id != *vault_id {
            return Err(ErrorCode::DeviceNotAuthorized);
        }
        // Replay/idempotency: byte-identical replay → stored result;
        // anything else for a passed generation → conflict (RF-05).
        if let Some(done) = head.finalized.get(&body.expected_old_generation) {
            if *done == sha_hex(body_bytes) {
                let m = SignedManifest::decode(&body.new_manifest)?;
                return Ok(FinalizeResult { generation: m.generation, manifest_hash: m.hash() });
            }
            return Err(ErrorCode::FinalizeConflict);
        }
        // 3: CAS preconditions, byte-exact.
        if body.expected_old_generation != head.generation
            || hex::encode(body.expected_old_manifest_hash) != head.manifest_hash
            || hex::encode(body.expected_old_registry_head) != head.registry_head
        {
            return Err(ErrorCode::FinalizeConflict);
        }
        // 4: structural validation.
        let entry = RegistryEntry::decode_tlv(&body.recovery_epoch_entry).map_err(|_| ErrorCode::InvalidInput)?;
        entry.validate_presence().map_err(|_| ErrorCode::InvalidInput)?;
        if entry.kind != EntryKind::RecoveryEpoch
            || entry.epoch != head.registry_epoch + 1
            || entry.vault_id != Some(*vault_id)
            || hex::encode(entry.prev_hash) != head.registry_head
            || registry::entry_hash(&entry).map_err(|_| ErrorCode::InvalidInput)? != body.new_registry_head
        {
            return Err(ErrorCode::InvalidInput);
        }
        let m = SignedManifest::decode(&body.new_manifest)?;
        let old = SignedManifest::decode(&self.read_object(vault_id, head.manifest_key.as_deref().ok_or(ErrorCode::BackupUnavailable)?)?)?;
        if m.generation != body.expected_old_generation + 1
            || m.registry_head != body.new_registry_head
            || m.vault_id != *vault_id
            || m.prev_manifest_hash != body.expected_old_manifest_hash
            || m.signer_device_id != entry.device_id
            || m.vk_generation != body.new_vk_generation
            || body.new_vk_generation != old.vk_generation + 1
        {
            return Err(ErrorCode::InvalidInput);
        }
        m.verify(&entry.sign_pub.ok_or(ErrorCode::InvalidInput)?)?; // RF-04
        // §4.8: the new state must arrive with a checkpoint describing it.
        super::fs_store::check_checkpoint_binding(&body.new_checkpoint, &m, entry.epoch)?;
        // 5: every referenced object already uploaded; registry = old + entry.
        let idx = self.check_index(vault_id, &m)?;
        let entries = registry_file::decode(&self.read_object(vault_id, &idx.registry.key)?)?;
        if entries.last() != Some(&entry) {
            return Err(ErrorCode::InvalidInput);
        }
        if fault == Some(FinalizeFault::CrashBeforeCommit) {
            return Err(ErrorCode::BackupUnavailable); // nothing saved (RF-06)
        }
        // Atomic commit: head + registry + device credential + result.
        let key = index::meta_key("manifest", &body.new_manifest);
        self.put_raw(vault_id, &key, &body.new_manifest)?;
        let cp_key = super::checkpoint::RegistryCheckpoint::key(m.generation);
        self.put_raw(vault_id, &cp_key, &body.new_checkpoint)?;
        head.checkpoint_key = Some(cp_key);
        self.advance(&mut head, key, &m, entry.epoch);
        head.device_creds.insert(
            hex::encode(entry.device_id),
            super::fs_store::DeviceCred { cred_sha: sha_hex(&body.new_device_backup_credential), active: true },
        );
        head.finalized.insert(body.expected_old_generation, sha_hex(body_bytes));
        self.save(vault_id, &head)?;
        Ok(FinalizeResult { generation: m.generation, manifest_hash: m.hash() })
    }
}
