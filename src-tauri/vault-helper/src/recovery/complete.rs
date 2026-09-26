//! Total-loss completion (spec v0.4 §11.8 "Client side"): materialize the
//! verified state, create the `recovery_epoch` and one `revoke` per prior
//! active device (§4.4 S-4, authorized by the new device), generate a
//! fresh VK and re-encrypt everything under it, write the new wraps and
//! the new device's envelope, and stage the `finalize` transition. The old
//! VK is zeroized once the re-encryption completes.

use std::path::Path;

use vault_proto::state::{RecoveryAuthEntry, TransitionKind};

use super::total_loss::Recovery;
use crate::crypto::recovery_auth::{self, RecoveryClass};
use crate::crypto::secret::{random_secret, SecretBytes};
use crate::device::rotate::EnvelopePlan;
use crate::errors::ErrorCode;
use crate::registry::build;
use crate::registry::chain::{self, EpochPolicy};
use crate::registry::device::DeviceIdentity;
use crate::registry::log;
use crate::storage::header::kdf_params;
use crate::storage::merge::{apply_batch, NoCompare};
use crate::storage::rotation::{self, MpWrap, RkWrap};
use crate::storage::store::{write_atomic, PASSWORD_WRAP_NAME, RECOVERY_WRAP_NAME};
use crate::storage::{rev_state, VaultStore};
use crate::sync::publish::Staging;

pub struct Plan<'a> {
    /// RK path: the new master password the user just set (required).
    pub new_mp: Option<&'a [u8]>,
    /// MP path: the user's existing RK, to keep it (proved against the
    /// wrap). Without it a new RK is issued.
    pub keep_rk: Option<&'a SecretBytes<32>>,
}

pub struct Completed {
    pub store: VaultStore,
    pub vk: SecretBytes<32>,
    pub staging: Staging,
    /// A newly issued RK, for the helper's own acknowledgement-gated sheet
    /// window (never over IPC).
    pub new_rk: Option<SecretBytes<32>>,
}

impl Recovery {
    /// Complete into the empty directory `dir`; on failure it is removed
    /// and the provider's current state stays authoritative.
    pub fn complete(&mut self, dir: &Path, new_device: &dyn DeviceIdentity, plan: Plan<'_>) -> Result<Completed, ErrorCode> {
        let r = self.complete_inner(dir, new_device, plan);
        if r.is_err() {
            let _ = std::fs::remove_dir_all(dir);
        }
        r
    }

    fn complete_inner(&mut self, dir: &Path, new_device: &dyn DeviceIdentity, plan: Plan<'_>) -> Result<Completed, ErrorCode> {
        let v = self.verified.take().ok_or(ErrorCode::BadState)?;
        let old_vk = self.old_vk.take().ok_or(ErrorCode::BadState)?;
        let vid = self.locate.vault_id;
        // Materialize the committed state (ciphertext only).
        std::fs::create_dir_all(dir).map_err(|_| ErrorCode::Internal)?;
        let mut store = VaultStore::create(dir, v.header.clone())?;
        {
            let gen = store.header.vk_generation;
            let tx = store.conn.transaction().map_err(|_| ErrorCode::DbCorrupt)?;
            apply_batch(&tx, &v.rows, gen, &NoCompare)?;
            // The index is ancestor-closed: anything held back means the
            // state is not what it claims. Rejected-and-counted revisions
            // do not fail the restore (§3.2; review SEC-I11).
            if rev_state::pending_count(&tx)? != 0 {
                return Err(ErrorCode::ManifestMismatch);
            }
            tx.commit().map_err(|_| ErrorCode::DbCorrupt)?;
        }
        store.persist_head()?;
        write_atomic(&dir.join(PASSWORD_WRAP_NAME), &v.wrap_mp)?;
        if let Some(rk) = &v.wrap_rk {
            write_atomic(&dir.join(RECOVERY_WRAP_NAME), rk)?;
        }
        // Registry: epoch bound to the recovered manifest, then S-4.
        let mut entries = v.entries.clone();
        entries.push(build::recovery_epoch(&v.registry, vid, v.remote.manifest_hash, &old_vk, new_device)?);
        let mut prior: Vec<[u8; 16]> = v.registry.devices.iter().filter(|d| !d.revoked).map(|d| d.device_id).collect();
        prior.sort();
        for id in &prior {
            let st = chain::verify_chain_with(&entries, &vid, &EpochPolicy::CheckpointAnchored)?;
            entries.push(build::revoke(&st, new_device, *id)?);
        }
        let registry = chain::verify_chain_with(&entries, &vid, &EpochPolicy::CheckpointAnchored)?;
        log::write_all(dir, &entries)?;
        store.set_registry_head(registry.head)?;
        store.set_author_device(&new_device.device_id())?;
        // Fresh VK; new wraps; the new device's first envelope.
        let kept_rk = match (&self.rk, plan.keep_rk) {
            (Some(rk), _) => Some(SecretBytes::new(*rk.expose())),
            (None, Some(rk)) => {
                let f: crate::crypto::wrap::RecoveryWrapFile =
                    serde_json::from_slice(v.wrap_rk.as_ref().ok_or(ErrorCode::RecoveryKeyInvalid)?).map_err(|_| ErrorCode::WrapCorrupt)?;
                crate::crypto::wrap::open_wrap_rk(&f, rk, &vid).map_err(|_| ErrorCode::RecoveryKeyInvalid)?;
                Some(SecretBytes::new(*rk.expose()))
            }
            (None, None) => None,
        };
        let new_rk = kept_rk.is_none().then(random_secret);
        let rk_for_wrap = kept_rk.as_ref().or(new_rk.as_ref()).ok_or(ErrorCode::Internal)?;
        let mp_plan = match (&self.pk, plan.new_mp) {
            (_, Some(mp)) => MpWrap::Fresh(mp),
            (Some(pk), None) => MpWrap::Reseal(pk),
            (None, None) => return Err(ErrorCode::InvalidInput), // RK path needs a new MP
        };
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).map_err(|_| ErrorCode::Internal)?;
        let env_plan = EnvelopePlan {
            vault_id: vid,
            devices: Vec::new(),
            fresh: vec![(new_device.device_id(), new_device.agree_pub(), nonce)],
        };
        // A new RK re-keys the RK class (§11.4 table): `SealNew`.
        let rk_plan = if new_rk.is_some() { RkWrap::SealNew(rk_for_wrap) } else { RkWrap::Seal(rk_for_wrap) };
        let rotated = rotation::rotate(store, &old_vk, mp_plan, rk_plan, Some(&env_plan), None)?;
        drop(old_vk);
        let store = VaultStore::open(dir)?;
        // Recovery-auth updates for every class whose key changed.
        let mut updates: Vec<RecoveryAuthEntry> = Vec::new();
        if let Some(mp) = plan.new_mp {
            let pk = crate::crypto::kdf::derive_pk(mp, &store.header.kdf.salt.0, kdf_params(&store.header.kdf)).map_err(|_| ErrorCode::Internal)?;
            let k = recovery_auth::derive(RecoveryClass::Mp, &pk, &store.header.auth_salt_mp.0, &vid).map_err(|_| ErrorCode::Internal)?;
            updates.push(RecoveryAuthEntry { class: RecoveryClass::Mp, public: k.public, salt: store.header.auth_salt_mp.0 });
        }
        if let Some(rk) = &new_rk {
            let k = recovery_auth::derive(RecoveryClass::Rk, rk, &store.header.auth_salt_rk.0, &vid).map_err(|_| ErrorCode::Internal)?;
            updates.push(RecoveryAuthEntry { class: RecoveryClass::Rk, public: k.public, salt: store.header.auth_salt_rk.0 });
        }
        let staged = crate::sync::local::stage_local(&store, &registry, v.remote.generation + 1, v.remote.manifest_hash, new_device, &rotated.new_vk, &[])?;
        let base_auth = v.remote.recovery_auth.clone();
        let staging = crate::sync::publish::finish_transition(TransitionKind::Finalize, staged, v.remote.state_commit, &base_auth, updates, None)?;
        Ok(Completed { store, vk: rotated.new_vk, staging, new_rk })
    }
}
