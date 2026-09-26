//! Building one publishable vault state (spec v0.4 §11.2–§11.3): the
//! index v2 over every blob the state references, the signed manifest v2,
//! the §4.8 checkpoint under the current VK, and the `StateTransition`
//! body. Shared by the helper (real states) and provider tests (synthetic
//! states) so there is one construction.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::checkpoint::RegistryCheckpoint;
use super::index::{IndexEntry, ObjectIndex, Role};
use super::manifest::SignedManifest;
use super::object;
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::registry::device::DeviceIdentity;
use crate::rev::{uuid_bytes, RevisionRow};
use crate::state::{RecoveryAuthEntry, StateTransition, TransitionKind};

/// Everything a state references, as serialized bytes (records as rows).
pub struct StateInputs<'a> {
    pub vault_id: [u8; 16],
    /// The generation being published (current + 1; 1 for `create`).
    pub generation: u64,
    pub prev_manifest_hash: [u8; 32],
    pub created_at: u64,
    pub header: Vec<u8>,
    pub registry: Vec<u8>,
    pub registry_head: [u8; 32],
    pub epoch: u64,
    pub vk_generation: u32,
    pub wrap_mp: Vec<u8>,
    pub wrap_rk: Option<Vec<u8>>,
    pub envelopes: Vec<([u8; 16], Vec<u8>)>,
    pub revisions: &'a [RevisionRow],
}

pub struct Staged {
    pub index: ObjectIndex,
    pub index_bytes: Vec<u8>,
    pub manifest: SignedManifest,
    pub checkpoint: RegistryCheckpoint,
    /// Every blob the state references, including the index, by SHA-256.
    pub blobs: BTreeMap<[u8; 32], Vec<u8>>,
}

pub fn stage(inp: StateInputs<'_>, signer: &dyn DeviceIdentity, vk: &SecretBytes<32>) -> Result<Staged, ErrorCode> {
    let mut blobs = BTreeMap::new();
    let mut entries = Vec::new();
    let mut add = |role: Role, bytes: Vec<u8>| {
        let e = IndexEntry::of(role, &bytes);
        blobs.insert(e.blob, bytes);
        entries.push(e);
    };
    add(Role::Header, inp.header);
    add(Role::Registry, inp.registry);
    add(Role::WrapMp, inp.wrap_mp);
    if let Some(rk) = inp.wrap_rk {
        add(Role::WrapRk, rk);
    }
    for (device_id, env) in inp.envelopes {
        add(Role::Env { device_id }, env);
    }
    let mut live = 0u64;
    for row in inp.revisions {
        let record_id = uuid_bytes(&row.record_id).ok_or(ErrorCode::DbCorrupt)?;
        add(Role::Rev { record_id, revision_id: row.revision_id, parents: row.parent_ids.clone() }, object::encode(row)?);
        live += u64::from(!row.deleted);
    }
    let index = ObjectIndex { generation: inp.generation, item_count: live, entries };
    let index_bytes = index.encode();
    let index_hash: [u8; 32] = Sha256::digest(&index_bytes).into();
    blobs.insert(index_hash, index_bytes.clone());
    let manifest = SignedManifest {
        vault_id: inp.vault_id,
        generation: inp.generation,
        created_at: inp.created_at,
        registry_head: inp.registry_head,
        vk_generation: inp.vk_generation,
        object_index_hash: index_hash,
        prev_manifest_hash: inp.prev_manifest_hash,
        signer_device_id: [0u8; 16],
        signature: [0u8; 64],
    }
    .sign(signer)?;
    let checkpoint = RegistryCheckpoint::create(vk, &manifest, inp.epoch)?;
    Ok(Staged { index, index_bytes, manifest, checkpoint, blobs })
}

/// Live-record count for FR-01 when revisions are not all at hand.
pub fn live_count(rows: &[RevisionRow]) -> u64 {
    rows.iter().filter(|r| !r.deleted).count() as u64
}

impl Staged {
    /// The §11.3 body. `create` carries every blob inline and the handle.
    pub fn transition(
        &self,
        kind: TransitionKind,
        expected_state: [u8; 32],
        updates: Vec<RecoveryAuthEntry>,
        handle_key: Option<[u8; 32]>,
    ) -> StateTransition {
        let create = kind == TransitionKind::Create;
        StateTransition {
            vault_id: self.manifest.vault_id,
            kind,
            expected_state,
            manifest: self.manifest.encode(),
            checkpoint: self.checkpoint.encode(),
            recovery_auth_updates: updates,
            handle_key: if create { handle_key } else { None },
            bootstrap_blobs: if create { self.blobs.values().cloned().collect() } else { Vec::new() },
        }
    }

    pub fn checkpoint_hash(&self) -> [u8; 32] {
        Sha256::digest(self.checkpoint.encode()).into()
    }
}
