//! Total-loss recovery over the provider protocol (spec v0.4 §11.5,
//! §11.8, §12 scenarios 3/4) in the corrected order:
//!
//! `KDF policy → derive → state_get → verify (VK, checkpoint, registry,
//! signature, header cross-check) → preview (FR-01) → recovery_epoch +
//! prior-device revokes → fresh VK, re-encrypt → stage finalize`
//!
//! The helper never touches the network: main moves the signed requests
//! and the bytes. MP/PK/RK/VK and the recovery-auth private key never
//! leave this module; there is no post-finalize rotation.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use super::locate::{self, LocateInfo};
use super::sheet;
use crate::backup::index::{ObjectIndex, Role};
use crate::backup::object;
use crate::crypto::kdf;
use crate::crypto::recovery_auth::{self, RecoveryAuthKey, RecoveryClass};
use crate::crypto::registry::RegistryEntry;
use crate::crypto::secret::SecretBytes;
use crate::crypto::wrap::{self, PasswordWrapFile, RecoveryWrapFile};
use crate::errors::ErrorCode;
use crate::registry::chain::{self, EpochPolicy, RegistryState};
use crate::registry::file as registry_file;
use crate::storage::header::{check_provider, kdf_params, parse_header, Header};
use crate::storage::revisions::{uuid_string, RevisionRow};
use crate::sync::remote::RemoteState;
use crate::sync::sign::{self, Key, SignRequest, SignScope};

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

/// A verified committed state ready for completion.
pub struct Verified {
    pub remote: RemoteState,
    pub header: Header,
    pub registry: RegistryState,
    pub entries: Vec<RegistryEntry>,
    pub rows: Vec<RevisionRow>,
    pub wrap_mp: Vec<u8>,
    pub wrap_rk: Option<Vec<u8>>,
    pub item_count: u64,
}

pub struct Recovery {
    pub origin: String,
    pub locate: LocateInfo,
    pub class: RecoveryClass,
    pub(super) pk: Option<SecretBytes<32>>,
    pub(super) rk: Option<SecretBytes<32>>,
    pub(super) key: RecoveryAuthKey,
    pub(super) old_vk: Option<SecretBytes<32>>,
    pub verified: Option<Verified>,
    pub created_at: u64,
}

impl Recovery {
    /// §11.5: the origin must be allowlisted and the locate response must
    /// pass the KDF policy before anything is derived from the secret.
    pub fn begin(origin: &str, locate_json: &[u8], cred: Credential<'_>, now: u64) -> Result<Recovery, ErrorCode> {
        check_provider(origin)?;
        let locate = locate::parse(locate_json)?;
        let (class, pk, rk, key) = match cred {
            Credential::Mp(mp) => {
                let pk = kdf::derive_pk(mp, &locate.kdf.salt.0, kdf_params(&locate.kdf)).map_err(|_| ErrorCode::Internal)?;
                let key = recovery_auth::derive(RecoveryClass::Mp, &pk, &locate.auth_salt_mp, &locate.vault_id).map_err(|_| ErrorCode::Internal)?;
                (RecoveryClass::Mp, Some(pk), None, key)
            }
            Credential::Rk(rk) => {
                let rk = SecretBytes::new(*rk.expose());
                let key = recovery_auth::derive(RecoveryClass::Rk, &rk, &locate.auth_salt_rk, &locate.vault_id).map_err(|_| ErrorCode::Internal)?;
                (RecoveryClass::Rk, None, Some(rk), key)
            }
        };
        Ok(Recovery { origin: origin.to_string(), locate, class, pk, rk, key, old_vk: None, verified: None, created_at: now })
    }

    /// Sign a recovery-class request (§11.4 policy: state/blob reads, and
    /// once completion staged them, its blob uploads and its finalize).
    pub fn sign(&self, req: &SignRequest, scope: &SignScope<'_>, now: u64) -> Result<String, ErrorCode> {
        sign::sign(&self.origin, self.locate.vault_id, Key::Recovery(&self.key, self.class), req, scope, self.created_at, now).map(|(_, h)| h)
    }

    /// The index blob to fetch for an offered state (every blob follows).
    pub fn plan(&self, remote: &RemoteState, index_bytes: &[u8]) -> Result<ObjectIndex, ErrorCode> {
        if remote.manifest.vault_id != self.locate.vault_id {
            return Err(ErrorCode::RecoveryMetadataMismatch);
        }
        let index = ObjectIndex::decode(index_bytes).map_err(|_| ErrorCode::ManifestMismatch)?;
        if index.hash() != remote.manifest.object_index_hash || index.generation != remote.generation {
            return Err(ErrorCode::ManifestMismatch);
        }
        index.check_structure().map_err(|_| ErrorCode::ManifestMismatch)?;
        Ok(index)
    }

    /// §4.8 order: recover the VK from the wrap → the current-VK
    /// checkpoint binds this registry head and manifest → only then is the
    /// registry the anchor → manifest signature → header cross-check.
    pub fn verify(&mut self, remote: RemoteState, index: &ObjectIndex, blobs: &HashMap<[u8; 32], Vec<u8>>) -> Result<Preview, ErrorCode> {
        let vid = self.locate.vault_id;
        let get = |role: &Role| -> Result<Vec<u8>, ErrorCode> {
            let e = index.find(role).ok_or(ErrorCode::ManifestMismatch)?;
            let b = blobs.get(&e.blob).ok_or(ErrorCode::BackupObjectMissing)?;
            if <[u8; 32]>::from(Sha256::digest(b)) != e.blob {
                return Err(ErrorCode::BackupObjectMissing);
            }
            Ok(b.clone())
        };
        let wrap_mp = get(&Role::WrapMp)?;
        let wrap_rk = index.find(&Role::WrapRk).map(|_| get(&Role::WrapRk)).transpose()?;
        // Wrong MP/RK → no key opens the wrap → WRONG_CREDENTIAL.
        let old_vk = match (&self.pk, &self.rk) {
            (Some(pk), _) => {
                let f: PasswordWrapFile = serde_json::from_slice(&wrap_mp).map_err(|_| ErrorCode::WrapCorrupt)?;
                wrap::open_wrap_mp(&f, pk, &vid).map_err(|_| ErrorCode::WrongCredential)?.vk
            }
            (None, Some(rk)) => {
                let f: RecoveryWrapFile = serde_json::from_slice(wrap_rk.as_ref().ok_or(ErrorCode::WrongCredential)?).map_err(|_| ErrorCode::WrapCorrupt)?;
                wrap::open_wrap_rk(&f, rk, &vid).map_err(|_| ErrorCode::WrongCredential)?.vk
            }
            _ => return Err(ErrorCode::Internal),
        };
        let entries = registry_file::decode(&get(&Role::Registry)?)?;
        let epoch = entries.last().map_or(0, |e| e.epoch);
        remote.checkpoint.verify_binding(&old_vk, &remote.manifest, &remote.manifest.registry_head, epoch)?;
        let registry = chain::verify_chain_with(&entries, &vid, &EpochPolicy::CheckpointAnchored)?;
        if registry.head != remote.manifest.registry_head || registry.epoch != epoch {
            return Err(ErrorCode::ManifestMismatch);
        }
        let signer = registry.active_device(&remote.manifest.signer_device_id).ok_or(ErrorCode::DeviceNotAuthorized)?;
        remote.manifest.verify(&signer.sign_pub)?;
        let header = parse_header(&get(&Role::Header)?)?;
        locate::cross_check(&self.locate, &header)?;
        if header.vk_generation != remote.vk_generation {
            return Err(ErrorCode::ManifestMismatch);
        }
        let mut rows = Vec::new();
        for e in index.revs() {
            let Role::Rev { record_id, revision_id, .. } = &e.role else { continue };
            let b = blobs.get(&e.blob).ok_or(ErrorCode::BackupObjectMissing)?;
            rows.push(object::decode_named(b, &e.blob, &uuid_string(record_id), revision_id)?);
        }
        let m = &remote.manifest;
        let preview = Preview {
            vault_id: m.vault_id,
            generation: m.generation,
            created_at: m.created_at,
            item_count: index.item_count,
            registry_head_prefix: sheet::head_prefix(&m.registry_head),
        };
        self.old_vk = Some(old_vk);
        self.verified = Some(Verified { item_count: index.item_count, remote, header, registry, entries, rows, wrap_mp, wrap_rk });
        Ok(preview)
    }
}
