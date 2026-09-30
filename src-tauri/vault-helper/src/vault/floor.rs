//! Rollback floor (§2.8) and the read-only `behind` mode (§22.14, review
//! SEC-B2/B3, VER-B1/B2).
//!
//! The Keychain floor records, for one vault, the highest local generation
//! this helper saw and the provider state it last accepted. A store below
//! it opens read-only. It catches up only through the provider: once the
//! accepted provider generation reaches the floor's (a sync applied, or
//! the provider has nothing newer), the local generation is raised to the
//! floor — the floor itself never goes down. While behind, the restored
//! store's own registry is not trusted: a served state must contain the
//! registry head the floor recorded, and fork evidence is not acted on.

use crate::crypto::hex;
use crate::errors::ErrorCode;
use crate::keychain::{self, Floor};
use crate::storage::VaultStore;
use crate::sync::{fetch, seen};

/// The floor for this store's vault (another vault's floor counts as none).
fn floor_for(store: &VaultStore) -> Result<Floor, ErrorCode> {
    let f = keychain::read_floor()?;
    let vid = hex::encode(store.header.vault_id.0);
    Ok(if f.vault_id.as_deref().is_some_and(|v| v != vid) { Floor::default() } else { f })
}

fn provider_generation(store: &VaultStore) -> Result<u64, ErrorCode> {
    Ok(seen::load(&store.conn)?.map_or(0, |s| s.generation))
}

/// §22.14 at unlock: this store is older than what this helper has seen.
pub fn behind(store: &VaultStore) -> Result<bool, ErrorCode> {
    let f = floor_for(store)?;
    let accepted = provider_generation(store)?;
    let provider_behind = f.provider_generation.is_some_and(|p| accepted < p);
    let same_provider = f.provider_generation.is_none_or(|p| accepted == p);
    let local_behind = same_provider && f.manifest_generation.is_some_and(|l| l > store.header.manifest_generation);
    Ok(provider_behind || local_behind)
}

/// Record this store as seen: never lowers anything. Called only when the
/// store is not behind.
pub fn raise(store: &VaultStore) -> Result<(), ErrorCode> {
    let mut f = floor_for(store)?;
    let before = f.clone();
    f.vault_id = Some(hex::encode(store.header.vault_id.0));
    f.manifest_generation = Some(f.manifest_generation.unwrap_or(0).max(store.header.manifest_generation));
    if let Some(s) = seen::load(&store.conn)? {
        if f.provider_generation.is_none_or(|p| s.generation > p) {
            f.provider_generation = Some(s.generation);
            f.state_commit = Some(hex::encode(s.state_commit.0));
            f.registry_head = Some(hex::encode(fetch::confirmed_registry(store)?.head));
        }
    }
    if f != before {
        keychain::write_floor(&f)?;
    }
    Ok(())
}

/// A fresh vault on this Mac (setup, total-loss recovery): the floor
/// starts over for it.
pub fn reset(store: &VaultStore) -> Result<(), ErrorCode> {
    let mut f = Floor { vault_id: Some(hex::encode(store.header.vault_id.0)), ..Floor::default() };
    f.manifest_generation = Some(store.header.manifest_generation);
    keychain::write_floor(&f)?;
    raise(store)
}

/// After a verified provider exchange while behind: caught up once the
/// accepted provider state reaches the floor's (same commitment at equal
/// generation). Raises the local generation to the floor, then the floor.
pub fn catch_up(store: &mut VaultStore) -> Result<bool, ErrorCode> {
    let f = floor_for(store)?;
    let s = seen::load(&store.conn)?;
    let accepted = s.as_ref().map_or(0, |s| s.generation);
    if let Some(p) = f.provider_generation {
        if accepted < p {
            return Ok(false);
        }
        let same_commit = s.as_ref().map(|s| hex::encode(s.state_commit.0)) == f.state_commit;
        if accepted == p && !same_commit {
            return Ok(false);
        }
    }
    store.raise_generation(f.manifest_generation.unwrap_or(0))?;
    raise(store)?;
    Ok(true)
}

/// While behind: a served registry must contain the head the floor
/// recorded, so a registry older than one this helper accepted — e.g.
/// one missing a later revocation — is never adopted.
pub fn anchored(store: &VaultStore, served: &[vault_proto::crypto::registry::RegistryEntry]) -> Result<bool, ErrorCode> {
    let Some(head) = floor_for(store)?.registry_head.and_then(|h| hex::decode_array::<32>(&h)) else {
        return Ok(true);
    };
    for e in served {
        if vault_proto::crypto::registry::entry_hash(e).map_err(|_| ErrorCode::Internal)? == head {
            return Ok(true);
        }
    }
    Ok(false)
}
