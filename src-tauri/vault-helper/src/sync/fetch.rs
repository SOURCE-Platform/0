//! Deciding what to fetch for a provider state (spec v0.4 §11.5 steps
//! 1–2): the rollback floor and fork check against the last accepted
//! state, then — once the index blob arrives and verifies — the blobs
//! this device does not already hold.

use std::collections::{BTreeSet, HashSet};

use super::remote::RemoteState;
use super::seen;
use crate::backup::index::{ObjectIndex, Role};
use crate::backup::object;
use crate::errors::ErrorCode;
use crate::storage::revision_rows::all_rows;
use crate::storage::VaultStore;

#[derive(Debug, PartialEq, Eq)]
pub enum Offer {
    /// Identical to the last accepted state.
    UpToDate,
    /// Fetch this blob (the index) next.
    Index([u8; 32]),
}

/// §11.5 step 1: lower generation → `MANIFEST_ROLLBACK`; the same
/// generation with a different manifest → fork evidence (§4.6).
pub fn offer(store: &VaultStore, remote: &RemoteState) -> Result<Offer, ErrorCode> {
    if remote.manifest.vault_id != store.header.vault_id.0 {
        return Err(ErrorCode::ManifestMismatch);
    }
    if let Some(s) = seen::load(&store.conn)? {
        if remote.generation < s.generation {
            return Err(ErrorCode::ManifestRollback);
        }
        if remote.generation == s.generation {
            return if remote.manifest_hash == s.manifest_hash.0 { Ok(Offer::UpToDate) } else { Err(ErrorCode::RegistryFork) };
        }
    }
    Ok(Offer::Index(remote.manifest.object_index_hash))
}

/// §11.5 step 2: verify the index against the manifest and list the
/// blobs to fetch — every singleton, every envelope, and every revision
/// blob not held locally in exactly that form.
pub fn plan(store: &VaultStore, remote: &RemoteState, index_bytes: &[u8]) -> Result<(ObjectIndex, Vec<[u8; 32]>), ErrorCode> {
    let index = ObjectIndex::decode(index_bytes).map_err(|_| ErrorCode::ManifestMismatch)?;
    if index.hash() != remote.manifest.object_index_hash || index.generation != remote.generation {
        return Err(ErrorCode::ManifestMismatch);
    }
    index.check_structure().map_err(|_| ErrorCode::ManifestMismatch)?;
    let held: HashSet<[u8; 32]> = all_rows(&store.conn)?
        .iter()
        .filter_map(|r| object::encode(r).ok().map(|b| object::blob_hash(&b)))
        .collect();
    let mut need = BTreeSet::new();
    for e in &index.entries {
        let wanted = match &e.role {
            Role::Header | Role::Registry | Role::WrapMp | Role::WrapRk => true,
            // Every envelope: adopting the committed singletons needs the
            // whole active set, and this device's own yields a new VK.
            Role::Env { .. } => true,
            Role::Rev { .. } => !held.contains(&e.blob),
        };
        if wanted {
            need.insert(e.blob);
        }
    }
    Ok((index, need.into_iter().collect()))
}
