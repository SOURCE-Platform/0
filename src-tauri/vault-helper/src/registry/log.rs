//! `registry.json` as a file: read + verify, and append one entry
//! atomically (§4.1, §4.7).
//!
//! Every read verifies the whole chain before any caller sees it (§4.4:
//! "callers never apply a partially verified registry"), and every append
//! re-verifies the extended chain before it replaces the file — a helper
//! bug can therefore never persist a chain this device would itself
//! reject.

use std::path::{Path, PathBuf};

use super::chain::{self, EpochPolicy, RegistryState};
use super::file;
use crate::crypto::registry::RegistryEntry;
use crate::errors::ErrorCode;
use crate::storage::store::write_atomic;
use crate::VAULT_REGISTRY_NAME;

pub fn path(dir: &Path) -> PathBuf {
    dir.join(VAULT_REGISTRY_NAME)
}

pub fn read_entries(dir: &Path) -> Result<Vec<RegistryEntry>, ErrorCode> {
    let bytes = std::fs::read(path(dir)).map_err(|_| ErrorCode::NotFound)?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    file::decode(&bytes)
}

/// Verified state of the on-disk registry. `policy` decides how
/// recovery_epoch entries are authorized (§4.4 rule 6 / §4.8).
pub fn read_state(
    dir: &Path,
    vault_id: &[u8; 16],
    policy: &EpochPolicy<'_>,
) -> Result<RegistryState, ErrorCode> {
    let entries = read_entries(dir)?;
    if entries.is_empty() {
        return Ok(RegistryState::empty());
    }
    chain::verify_chain_with(&entries, vault_id, policy)
}

/// Append one entry, re-verifying the extended chain first. The new file
/// is written atomically, so a crash leaves either the old chain or the
/// new one.
pub fn append(
    dir: &Path,
    vault_id: &[u8; 16],
    state: &RegistryState,
    entry: RegistryEntry,
    policy: &EpochPolicy<'_>,
) -> Result<RegistryState, ErrorCode> {
    let mut entries = state.entries.clone();
    entries.push(entry);
    let extended = chain::verify_chain_with(&entries, vault_id, policy)?;
    write_atomic(&path(dir), &file::encode(&entries)?)?;
    Ok(extended)
}

/// Write a whole chain (vault creation's genesis entry).
pub fn write_all(dir: &Path, entries: &[RegistryEntry]) -> Result<(), ErrorCode> {
    write_atomic(&path(dir), &file::encode(entries)?)
}
