//! §11.3 steps 4–8: structural validation of a `StateTransition` against
//! the current state. The provider is defense in depth, never a root of
//! trust; it holds no VK, so it checks the checkpoint's binding but not
//! its MAC. Any failure means no mutation.

use std::collections::HashMap;

use sha2::{Digest, Sha256};
use vault_proto::backup::checkpoint::RegistryCheckpoint;
use vault_proto::backup::index::{ObjectIndex, Role};
use vault_proto::backup::manifest::SignedManifest;
use vault_proto::crypto::recovery_auth::RecoveryClass;
use vault_proto::errors::ErrorCode;
use vault_proto::header::{mp_wrap_matches, parse_header, Hex16, Hex32};
use vault_proto::registry::file;
use vault_proto::state::{StateTransition, TransitionKind};

use crate::auth::Signer;
use crate::model::{ActiveDevice, Locate, Reject, VaultStateDoc};
use crate::{registry_rules, Provider};

pub const CAP_INDEX: u64 = 8 << 20;
pub const CAP_REGISTRY: u64 = 4 << 20;
pub const CAP_BLOB: u64 = 1 << 20;

/// A validated transition: the state to commit (claim fields and
/// bookkeeping filled in by the committer) and blobs to write first.
pub struct Validated {
    pub state: VaultStateDoc,
    pub blobs: Vec<([u8; 32], Vec<u8>)>,
}

pub fn role_cap(role: &Role) -> u64 {
    match role {
        Role::Registry => CAP_REGISTRY,
        _ => CAP_BLOB,
    }
}

impl Provider {
    pub(crate) fn validate(
        &self,
        cur: Option<&VaultStateDoc>,
        t: &StateTransition,
        signer: &Signer,
    ) -> Result<Validated, Reject> {
        let vid = t.vault_id;
        let inline: HashMap<[u8; 32], &Vec<u8>> =
            t.bootstrap_blobs.iter().map(|b| (Sha256::digest(b).into(), b)).collect();
        let fetch = |sha: &[u8; 32]| -> Result<Vec<u8>, ErrorCode> {
            if let Some(b) = inline.get(sha) {
                return Ok((*b).clone());
            }
            self.blobs.get(&vid, sha).map_err(|_| ErrorCode::BackupUnavailable)?.ok_or(ErrorCode::BlobMissing)
        };
        // Step 4: manifest.
        let m = SignedManifest::decode(&t.manifest).map_err(|_| ErrorCode::ManifestInvalid)?;
        let (gen, prev) = cur.map_or((0, [0u8; 32]), |c| (c.generation, c.manifest_hash.0));
        if m.vault_id != vid || m.generation != gen + 1 || m.prev_manifest_hash != prev {
            return Err(ErrorCode::ManifestInvalid.into());
        }
        // Step 5: index.
        let index_bytes = match fetch(&m.object_index_hash) {
            Err(ErrorCode::BlobMissing) => return Err(Reject(ErrorCode::BlobMissing, serde_json::json!({ "count": 1 }))),
            r => r?,
        };
        if index_bytes.len() as u64 > CAP_INDEX {
            return Err(ErrorCode::TooLarge.into());
        }
        let index = ObjectIndex::decode(&index_bytes).map_err(|_| ErrorCode::IndexInvalid)?;
        if index.generation != m.generation {
            return Err(ErrorCode::IndexInvalid.into());
        }
        index.check_structure()?;
        let mut missing = 0u64;
        for e in &index.entries {
            if e.size > role_cap(&e.role) {
                return Err(ErrorCode::TooLarge.into());
            }
            let size = match inline.get(&e.blob) {
                Some(b) => Some(b.len() as u64),
                None => self.blobs.size(&vid, &e.blob).map_err(|_| ErrorCode::BackupUnavailable)?,
            };
            match size {
                None => missing += 1,
                Some(s) if s != e.size => return Err(ErrorCode::IndexInvalid.into()),
                Some(_) => {}
            }
        }
        if missing > 0 {
            return Err(Reject(ErrorCode::BlobMissing, serde_json::json!({ "count": missing })));
        }
        if t.kind == TransitionKind::Create {
            // The inline blobs are exactly the index plus what it lists.
            let mut want = index.blobs();
            want.insert(m.object_index_hash);
            if inline.keys().copied().collect::<std::collections::BTreeSet<_>>() != want {
                return Err(ErrorCode::IndexInvalid.into());
            }
        }
        let entry = |r: Role| index.find(&r).map(|e| e.blob).ok_or(ErrorCode::IndexInvalid);
        // Step 6: registry.
        let registry_sha = entry(Role::Registry)?;
        let cur_entries = match cur {
            Some(c) => file::decode(&fetch(&c.registry_hash.0)?).map_err(|_| ErrorCode::Internal)?,
            None => Vec::new(),
        };
        let reg = registry_rules::check(t.kind, cur.map(|c| (c, cur_entries.as_slice())), &fetch(&registry_sha)?, &vid)?;
        if reg.state.head != m.registry_head {
            return Err(ErrorCode::RegistryInvalid.into());
        }
        let active: Vec<ActiveDevice> = reg
            .state
            .devices
            .iter()
            .filter(|d| !d.revoked)
            .map(|d| ActiveDevice { device_id: Hex16(d.device_id), sign_pub: d.sign_pub })
            .collect();
        let mut env_ids: Vec<[u8; 16]> = index.envs().map(|(id, _)| *id).collect();
        let mut active_ids: Vec<[u8; 16]> = active.iter().map(|d| d.device_id.0).collect();
        env_ids.sort();
        active_ids.sort();
        if env_ids != active_ids {
            return Err(ErrorCode::IndexInvalid.into());
        }
        let cur_vk = cur.map_or(0, |c| c.vk_generation);
        let vk_ok = match t.kind {
            TransitionKind::Create => m.vk_generation >= 1,
            TransitionKind::Publish if !reg.revoked.is_empty() => m.vk_generation == cur_vk + 1,
            TransitionKind::Publish => m.vk_generation == cur_vk || m.vk_generation == cur_vk + 1,
            TransitionKind::Finalize => m.vk_generation == cur_vk + 1,
        };
        if !vk_ok {
            return Err(ErrorCode::ManifestInvalid.into());
        }
        // Step 4 (signer binding) — needs the new registry for finalize.
        let signer_id = match (t.kind, signer) {
            (TransitionKind::Create, Signer::Device(id)) => (reg.state.entries[0].device_id == *id).then_some(*id),
            (TransitionKind::Publish, Signer::Device(id)) => Some(*id),
            (TransitionKind::Finalize, Signer::Recovery(_)) => {
                reg.state.entries.get(cur_entries.len()).map(|e| e.device_id)
            }
            _ => None,
        }
        .ok_or(ErrorCode::DeviceNotAuthorized)?;
        if m.signer_device_id != signer_id {
            return Err(ErrorCode::ManifestInvalid.into());
        }
        // Step 7: header and recovery-auth rules.
        let header_sha = entry(Role::Header)?;
        let h = parse_header(&fetch(&header_sha)?).map_err(|_| ErrorCode::ManifestInvalid)?;
        if h.vault_id.0 != vid || !h.kdf.is_allowlisted() || h.vk_generation != m.vk_generation {
            return Err(ErrorCode::ManifestInvalid.into());
        }
        let mut recovery_auth = cur.map(|c| c.recovery_auth.clone()).unwrap_or_default();
        check_recovery_auth(t, cur, &h, &reg.revoked, &fetch(&entry(Role::WrapMp)?)?)?;
        recovery_auth.apply(&t.recovery_auth_updates);
        // Step 8: signatures and bindings.
        let sign_pub = reg.state.devices.iter().find(|d| d.device_id == signer_id).map(|d| d.sign_pub);
        m.verify(&sign_pub.ok_or(ErrorCode::ManifestInvalid)?).map_err(|_| ErrorCode::ManifestInvalid)?;
        let cp = RegistryCheckpoint::decode(&t.checkpoint).map_err(|_| ErrorCode::CheckpointMismatch)?;
        let bound = cp.vault_id == vid
            && cp.registry_head == m.registry_head
            && cp.manifest_core_hash == m.core_hash()
            && cp.manifest_generation == m.generation
            && cp.vk_generation == m.vk_generation
            && cp.epoch == reg.state.epoch;
        if !bound {
            return Err(ErrorCode::CheckpointMismatch.into());
        }
        let manifest_hash = m.hash();
        let checkpoint_hash: [u8; 32] = Sha256::digest(&t.checkpoint).into();
        let mut blobs = vec![(manifest_hash, t.manifest.clone()), (checkpoint_hash, t.checkpoint.clone())];
        blobs.extend(inline.iter().map(|(k, v)| (*k, (*v).clone())));
        let state = VaultStateDoc {
            v: 2,
            vault_id: Hex16(vid),
            generation: m.generation,
            manifest_hash: Hex32(manifest_hash),
            checkpoint_hash: Hex32(checkpoint_hash),
            index_hash: Hex32(m.object_index_hash),
            registry_hash: Hex32(registry_sha),
            registry_head: Hex32(reg.state.head),
            registry_seq: reg.state.entries.len() as u64,
            epoch: reg.state.epoch,
            header_hash: Hex32(header_sha),
            vk_generation: m.vk_generation,
            active_devices: active,
            recovery_auth,
            locate: Locate { kdf: h.kdf.clone(), auth_salt_mp: h.auth_salt_mp, auth_salt_rk: h.auth_salt_rk },
            handle_key: cur.map_or(Hex32(t.handle_key.unwrap_or([0; 32])), |c| c.handle_key),
            claim_id: cur.map_or(Hex16([0; 16]), |c| c.claim_id),
            state_commit: Hex32([0; 32]),
            retained: Vec::new(),
            recent: Vec::new(),
            finalized: Default::default(),
        };
        Ok(Validated { state, blobs })
    }
}

/// Step 7 recovery-auth rules (owner decision D-11).
fn check_recovery_auth(
    t: &StateTransition,
    cur: Option<&VaultStateDoc>,
    h: &vault_proto::header::Header,
    revoked: &[[u8; 16]],
    mp_wrap: &[u8],
) -> Result<(), ErrorCode> {
    let stale = Err(ErrorCode::RecoveryAuthStale);
    let update = |c: RecoveryClass| t.recovery_auth_updates.iter().find(|u| u.class == c);
    for (class, salt) in [(RecoveryClass::Mp, h.auth_salt_mp), (RecoveryClass::Rk, h.auth_salt_rk)] {
        let changed = cur.is_none_or(|c| match class {
            RecoveryClass::Mp => c.locate.auth_salt_mp != salt,
            RecoveryClass::Rk => c.locate.auth_salt_rk != salt,
        });
        match update(class) {
            Some(u) if !changed || u.salt != salt.0 => return stale,
            None if changed => return stale,
            _ => {}
        }
    }
    let kdf_salt_changed = cur.is_none_or(|c| c.locate.kdf.salt != h.kdf.salt);
    if update(RecoveryClass::Mp).is_some() != kdf_salt_changed || !mp_wrap_matches(mp_wrap, &h.kdf) {
        return stale;
    }
    let both = update(RecoveryClass::Mp).is_some() && update(RecoveryClass::Rk).is_some();
    let needs_both = t.kind == TransitionKind::Create || (t.kind == TransitionKind::Publish && !revoked.is_empty());
    if needs_both && !both {
        return stale;
    }
    Ok(())
}
