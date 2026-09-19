//! Total-loss recovery (spec §12 scenarios 3/4, §11.8) in the corrected
//! order (v0.3 correction, Phase C.1):
//!
//! `recover old VK → create recovery_epoch → generate fresh VK →
//! re-encrypt current vault under fresh VK → build/upload new objects and
//! wraps → recovery-finalize installs already-rotated state → UNLOCKED`
//!
//! There is no second, post-finalize rotation. `begin` covers locate,
//! download, unwrap, and verification and yields the freshness preview
//! the UI must show **before** completion (FR-01); `complete` does the
//! rest. Runs inside the helper; MP/RK/VK never leave it.

use std::path::Path;

use super::creds::{self, RecoveryCreds};
use super::sheet::{self, SheetCheckpoint, SheetComparison};
use crate::backup::finalize::FinalizeBody;
use crate::backup::fs_recovery::{FinalizeFault, LocateInfo};
use crate::backup::fs_store::{Auth, FsBackupStore, RecoveryKind};
use crate::backup::snapshot::{self, Downloaded};
use crate::crypto::kdf::{self, Argon2Params};
use crate::crypto::registry::{self, RegistryEntry};
use crate::crypto::secret::{random_secret, SecretBytes};
use crate::crypto::wrap::{self, PasswordWrapFile, RecoveryWrapFile};
use crate::errors::ErrorCode;
use crate::registry::build;
use crate::registry::chain::{self, EpochContext, RegistryState};
use crate::registry::device::DeviceIdentity;
use crate::registry::file as registry_file;
use crate::storage::rotation::{self, MpWrap, RkWrap};
use crate::storage::store::write_atomic;
use crate::storage::VaultStore;
use crate::VAULT_REGISTRY_NAME;

/// What the user entered in the helper panel.
pub enum Credential<'a> {
    Mp(&'a [u8]),
    Rk(&'a SecretBytes<32>),
}

/// FR-01: shown before completion. Non-secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub vault_id: [u8; 16],
    pub generation: u64,
    pub created_at: u64,
    pub item_count: u64,
    pub registry_head_prefix: String,
}

pub struct RecoverySession<'b> {
    backup: &'b FsBackupStore,
    kind: RecoveryKind,
    creds: RecoveryCreds,
    locate: LocateInfo,
    pk: Option<SecretBytes<32>>,
    rk: Option<SecretBytes<32>>,
    old_vk: SecretBytes<32>,
    downloaded: Downloaded,
    registry: RegistryState,
}

/// Only the recovered manifest's VK is available on a fresh device.
struct FreshDeviceCtx<'a> {
    manifest_hash: [u8; 32],
    vk: &'a SecretBytes<32>,
}

impl EpochContext for FreshDeviceCtx<'_> {
    fn vk_for_manifest(&self, h: &[u8; 32]) -> Option<SecretBytes<32>> {
        (h == &self.manifest_hash).then(|| SecretBytes::new(*self.vk.expose()))
    }
    fn manifest_acceptable(&self, _h: &[u8; 32]) -> bool {
        true // no remembered state on a fresh device (§11.7)
    }
}

/// Steps 1–3: locate, derive recovery creds, download, unwrap, verify.
pub fn begin<'b>(backup: &'b FsBackupStore, email: &str, credential: Credential<'_>) -> Result<RecoverySession<'b>, ErrorCode> {
    let locate = backup.recover_locate(email).map_err(|_| ErrorCode::WrongCredential)?;
    let (kind, creds, pk, rk) = match credential {
        Credential::Mp(mp) => {
            let pk = kdf::derive_pk(mp, &locate.kdf_salt, Argon2Params::V1).map_err(|_| ErrorCode::Internal)?;
            (RecoveryKind::Mp, creds::mp_creds(&pk, &locate.locator_salt_mp)?, Some(pk), None)
        }
        Credential::Rk(rk) => {
            let rk = SecretBytes::new(*rk.expose());
            (RecoveryKind::Rk, creds::rk_creds(&rk, &locate.locator_salt_rk)?, None, Some(rk))
        }
    };
    // Wrong MP/RK → unknown locator → WRONG_CREDENTIAL, never a decryption result.
    let bundle = backup
        .recover_bundle(&locate.vault_id, &creds.locator, kind, creds.cred.expose())
        .map_err(|_| ErrorCode::WrongCredential)?;
    let auth = Auth::Recovery { kind, cred: creds.cred.expose() };
    let downloaded = snapshot::download(backup, &bundle.manifest, auth)?;
    let vault_id = downloaded.manifest.vault_id;
    let old_vk = match (&pk, &rk) {
        (Some(pk), _) => {
            let file: PasswordWrapFile = serde_json::from_slice(&downloaded.wrap_mp).map_err(|_| ErrorCode::WrapCorrupt)?;
            wrap::open_wrap_mp(&file, pk, &vault_id).map_err(|_| ErrorCode::WrongCredential)?.vk
        }
        (None, Some(rk)) => {
            let bytes = downloaded.wrap_rk.as_ref().ok_or(ErrorCode::WrongCredential)?;
            let file: RecoveryWrapFile = serde_json::from_slice(bytes).map_err(|_| ErrorCode::WrapCorrupt)?;
            wrap::open_wrap_rk(&file, rk, &vault_id).map_err(|_| ErrorCode::WrongCredential)?.vk
        }
        (None, None) => return Err(ErrorCode::Internal),
    };
    // Verify the registry chain and the manifest signature under it.
    let ctx = FreshDeviceCtx { manifest_hash: downloaded.manifest.hash(), vk: &old_vk };
    let registry = chain::verify_chain(&downloaded.registry, &vault_id, &ctx)?;
    if registry.head != downloaded.manifest.registry_head {
        return Err(ErrorCode::ManifestMismatch);
    }
    let signer = registry
        .active_device(&downloaded.manifest.signer_device_id)
        .ok_or(ErrorCode::DeviceNotAuthorized)?;
    downloaded.manifest.verify(&signer.sign_pub)?;
    Ok(RecoverySession { backup, kind, creds, locate, pk, rk, old_vk, downloaded, registry })
}

/// What the caller supplies at completion.
pub struct CompletePlan<'a> {
    /// RK path: the new master password the user just set (required).
    pub new_mp: Option<&'a [u8]>,
    /// MP path: the user's existing RK, if they have it, to keep it.
    /// Without it a new RK is generated (RK re-wrap needs RK_bytes).
    pub keep_rk: Option<&'a SecretBytes<32>>,
    pub fault: Option<FinalizeFault>,
    /// Test hook (RF-03/04/05): inspect or tamper with the finalize body
    /// before it is sent. Production callers pass `None`.
    pub on_body: Option<&'a dyn Fn(&mut FinalizeBody)>,
}

pub struct Recovered {
    pub vk: SecretBytes<32>,
    pub vk_generation: u32,
    pub generation: u64,
    pub device_cred: [u8; 32],
    /// Set when a new RK had to be generated: the caller shows/prints it
    /// in the helper panel (never over IPC).
    pub new_rk: Option<SecretBytes<32>>,
}

impl RecoverySession<'_> {
    pub fn preview(&self) -> Preview {
        let m = &self.downloaded.manifest;
        Preview {
            vault_id: m.vault_id,
            generation: m.generation,
            created_at: m.created_at,
            item_count: self.downloaded.index.item_count,
            registry_head_prefix: sheet::head_prefix(&m.registry_head),
        }
    }

    pub fn compare_sheet(&self, sheet: &SheetCheckpoint) -> SheetComparison {
        let m = &self.downloaded.manifest;
        sheet::compare(sheet, &m.vault_id, m.generation, &m.registry_head)
    }

    /// Steps 4–6 in the corrected order. `dir` must be empty; on any
    /// failure it is removed again and the provider's old head stays
    /// authoritative.
    pub fn complete(self, dir: &Path, new_device: &dyn DeviceIdentity, plan: CompletePlan<'_>) -> Result<Recovered, ErrorCode> {
        let result = self.complete_inner(dir, new_device, plan);
        if result.is_err() {
            let _ = std::fs::remove_dir_all(dir);
        }
        result
    }

    fn complete_inner(self, dir: &Path, new_device: &dyn DeviceIdentity, plan: CompletePlan<'_>) -> Result<Recovered, ErrorCode> {
        let old = &self.downloaded.manifest;
        let vault_id = old.vault_id;
        let old_hash = old.hash();
        // (a) recover old VK: done in `begin`. (b) create the recovery_epoch.
        let epoch = build::recovery_epoch(&self.registry, vault_id, old_hash, &self.old_vk, new_device)?;
        let mut entries: Vec<RegistryEntry> = self.downloaded.registry.clone();
        entries.push(epoch.clone());
        // (c)+(d) fresh VK, re-encrypt the current vault, new wraps.
        std::fs::create_dir_all(dir).map_err(|_| ErrorCode::Internal)?;
        let store = snapshot::materialize(dir, &self.downloaded)?;
        let mut new_rk = None;
        let mp_plan = match (&self.pk, plan.new_mp) {
            (_, Some(mp)) => MpWrap::Fresh(mp),
            (Some(pk), None) => MpWrap::Reseal(pk),
            (None, None) => return Err(ErrorCode::InvalidInput), // RK path needs a new MP
        };
        let kept_rk = match (&self.rk, plan.keep_rk) {
            (Some(rk), _) => Some(SecretBytes::new(*rk.expose())),
            (None, Some(rk)) => {
                // Prove the offered RK is this vault's RK before keeping it.
                let bytes = self.downloaded.wrap_rk.as_ref().ok_or(ErrorCode::RecoveryKeyInvalid)?;
                let file: RecoveryWrapFile = serde_json::from_slice(bytes).map_err(|_| ErrorCode::WrapCorrupt)?;
                wrap::open_wrap_rk(&file, rk, &vault_id).map_err(|_| ErrorCode::RecoveryKeyInvalid)?;
                Some(SecretBytes::new(*rk.expose()))
            }
            (None, None) => None,
        };
        let rk_for_wrap = match kept_rk {
            Some(rk) => rk,
            None => {
                let fresh = random_secret();
                new_rk = Some(SecretBytes::new(*fresh.expose()));
                fresh
            }
        };
        let rotated = rotation::rotate(store, &self.old_vk, mp_plan, RkWrap::Seal(&rk_for_wrap), None)?;
        drop(self.old_vk); // old VK zeroized once the re-encryption completed
        // (e) build + upload the rotated snapshot with the recovery credential.
        write_atomic(&dir.join(VAULT_REGISTRY_NAME), &registry_file::encode(&entries)?)?;
        let store = VaultStore::open(dir)?;
        let snap = snapshot::build(&store, &entries, old.generation + 1, old_hash, new_device)?;
        let auth = Auth::Recovery { kind: self.kind, cred: self.creds.cred.expose() };
        snapshot::upload(self.backup, &vault_id, &snap, auth)?;
        // (f) recovery-finalize installs the already-rotated state.
        let mut device_cred = [0u8; 32];
        getrandom::fill(&mut device_cred).expect("OS RNG");
        let body = FinalizeBody {
            vault_id,
            expected_old_generation: old.generation,
            expected_old_manifest_hash: old_hash,
            expected_old_registry_head: self.registry.head,
            recovery_credential_class: self.kind.class(),
            recovery_epoch_entry: epoch.encode_tlv(true).map_err(|_| ErrorCode::Internal)?,
            new_manifest: snap.manifest.encode(),
            new_registry_head: registry::entry_hash(&epoch).map_err(|_| ErrorCode::Internal)?,
            new_vk_generation: rotated.vk_generation,
            new_device_backup_credential: device_cred,
        };
        let mut body = body;
        if let Some(hook) = plan.on_body {
            hook(&mut body);
        }
        let done = self
            .backup
            .recovery_finalize(&vault_id, &body.encode(), self.kind, self.creds.cred.expose(), plan.fault)?;
        // (g) re-register recovery locators/creds that changed, as the
        // newly installed device (§11.4; BK-16).
        let dev_auth = Auth::Device { device_id: new_device.device_id(), cred: &device_cred };
        if let Some(mp) = plan.new_mp {
            let salt = store.header.kdf.salt_bytes()?;
            let pk = kdf::derive_pk(mp, &salt, Argon2Params::V1).map_err(|_| ErrorCode::Internal)?;
            let c = creds::mp_creds(&pk, &self.locate.locator_salt_mp)?;
            self.backup.update_kdf_salt(&vault_id, salt, dev_auth)?;
            self.backup.register_recovery(&vault_id, RecoveryKind::Mp, &c.locator, c.cred.expose(), dev_auth)?;
        }
        if new_rk.is_some() {
            let c = creds::rk_creds(&rk_for_wrap, &self.locate.locator_salt_rk)?;
            self.backup.register_recovery(&vault_id, RecoveryKind::Rk, &c.locator, c.cred.expose(), dev_auth)?;
        }
        // (h) UNLOCKED on the fresh VK — no further rotation.
        Ok(Recovered {
            vk: rotated.new_vk,
            vk_generation: rotated.vk_generation,
            generation: done.generation,
            device_cred,
            new_rk,
        })
    }
}
