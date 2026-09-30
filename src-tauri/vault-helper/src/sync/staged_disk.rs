//! On-disk staging of a fully staged publication (spec §22.11, F2-D5 #2).
//!
//! `staging/pending/` holds the transition body and every blob — the
//! ciphertext and public data the provider is about to receive, nothing
//! else — so a security cutoff survives a helper restart and can finish
//! while LOCKED. The directory is not the authority: the helper resumes
//! only what the `staged_publication` record in `vault.db` names, and any
//! new authority change deletes that record in its own commit
//! (`pending::add`), so stale staging can never outlive what supersedes it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use vault_proto::backup::manifest::SignedManifest;
use vault_proto::state::{recovery_auth_digest, state_commit, StateTransition};

use super::pending::{self, Base};
use super::publish::Staging;
use super::seen::{self, merge_auth};
use crate::crypto::hex;
use crate::errors::ErrorCode;
use crate::storage::kv;
use crate::storage::store::write_atomic;
use crate::storage::VaultStore;

pub const KEY: &str = "staged_publication";
const BODY: &str = "body";
const READY: &str = "ready.json";

pub fn dir(vault_dir: &Path) -> PathBuf {
    vault_dir.join("staging").join("pending")
}

#[derive(Serialize, Deserialize)]
struct Record {
    body_sha256: String,
    expected_state: String,
    carries: Option<(u64, Base)>,
}

/// Written last: the body hash and every blob's hash and size.
#[derive(Serialize, Deserialize)]
struct Ready {
    body_sha256: String,
    blobs: Vec<(String, u64)>,
}

/// Persist a fully staged publication. Files first, the descriptor last,
/// then the record that makes it resumable.
pub fn persist(store: &VaultStore, st: &Staging) -> Result<(), ErrorCode> {
    use std::os::unix::fs::PermissionsExt;
    let d = dir(&store.dir);
    forget(&store.dir, &store.conn)?;
    std::fs::create_dir_all(&d).map_err(|_| ErrorCode::Internal)?;
    for p in [d.parent().unwrap_or(&d), d.as_path()] {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700)).map_err(|_| ErrorCode::Internal)?;
    }
    for (sha, blob) in &st.blobs {
        write_atomic(&d.join(hex::encode(sha)), blob)?;
    }
    write_atomic(&d.join(BODY), &st.body)?;
    let ready = Ready { body_sha256: hex::encode(st.body_sha256), blobs: st.blobs.iter().map(|(h, b)| (hex::encode(h), b.len() as u64)).collect() };
    write_atomic(&d.join(READY), &serde_json::to_vec(&ready).map_err(|_| ErrorCode::Internal)?)?;
    let record = Record { body_sha256: hex::encode(st.body_sha256), expected_state: hex::encode(st.expected_state), carries: st.carries_pending.clone() };
    kv::put(&store.conn, KEY, &record)
}

/// Drop the record and the directory (commit, terminal failure, stale).
pub fn forget(vault_dir: &Path, conn: &Connection) -> Result<(), ErrorCode> {
    kv::delete(conn, KEY)?;
    let _ = std::fs::remove_dir_all(dir(vault_dir));
    Ok(())
}

/// Validate and rebuild the staged publication, or discard it. Nothing
/// here needs the VK.
pub fn load(store: &VaultStore) -> Result<Option<Staging>, ErrorCode> {
    let found = read(store);
    if !matches!(found, Ok(Some(_))) {
        forget(&store.dir, &store.conn)?;
    }
    Ok(found.unwrap_or(None))
}

fn read(store: &VaultStore) -> Result<Option<Staging>, ErrorCode> {
    let d = dir(&store.dir);
    let Some(record) = kv::get::<Record>(&store.conn, KEY)? else {
        return Ok(None);
    };
    let bad = ErrorCode::TransferInvalid;
    let ready: Ready = std::fs::read(d.join(READY)).ok().and_then(|b| serde_json::from_slice(&b).ok()).ok_or(bad)?;
    let body = std::fs::read(d.join(BODY)).map_err(|_| bad)?;
    let body_sha256: [u8; 32] = Sha256::digest(&body).into();
    if ready.body_sha256 != record.body_sha256 || hex::encode(body_sha256) != record.body_sha256 {
        return Err(bad);
    }
    let mut blobs = BTreeMap::new();
    for (name, size) in &ready.blobs {
        let sha = hex::decode_array::<32>(name).ok_or(bad)?;
        let blob = std::fs::read(d.join(name)).map_err(|_| bad)?;
        if blob.len() as u64 != *size || <[u8; 32]>::from(Sha256::digest(&blob)) != sha {
            return Err(bad);
        }
        blobs.insert(sha, blob);
    }
    // The base it was built on is still the committed one, and the
    // pending change it carries is still the current one.
    let seen = seen::load(&store.conn)?;
    let t = StateTransition::decode(&body).map_err(|_| bad)?;
    let committed = seen.as_ref().map_or([0u8; 32], |s| s.state_commit.0);
    let current = pending::load(&store.conn)?;
    let carried = record.carries.as_ref().map(|(v, _)| *v);
    let updates_match = match &current {
        Some(p) if carried.is_some() => {
            let own = seen::Seen { recovery_auth: p.recovery_auth_updates.clone(), ..seen.clone().unwrap_or_else(empty_seen) }.auth_entries()?;
            own.iter().all(|u| t.recovery_auth_updates.contains(u))
        }
        _ => true,
    };
    if t.expected_state != committed
        || hex::encode(t.expected_state) != record.expected_state
        || t.vault_id != store.header.vault_id.0
        || carried != current.as_ref().filter(|p| !p.needs_user).map(|p| p.version)
        || !updates_match
    {
        return Err(bad);
    }
    let manifest = SignedManifest::decode(&t.manifest).map_err(|_| bad)?;
    let base_auth = match &seen {
        Some(s) => s.auth_entries()?,
        None => Vec::new(),
    };
    let new_auth = merge_auth(&base_auth, &t.recovery_auth_updates);
    let manifest_hash = manifest.hash();
    let checkpoint_hash: [u8; 32] = Sha256::digest(&t.checkpoint).into();
    Ok(Some(Staging {
        kind: t.kind,
        blobs,
        body_sha256,
        expected_state: t.expected_state,
        generation: manifest.generation,
        manifest_hash,
        new_state_commit: state_commit(&manifest.vault_id, manifest.generation, &manifest_hash, &checkpoint_hash, &recovery_auth_digest(&new_auth)?),
        new_auth,
        carries_pending: record.carries,
        body_manifest: t.manifest.clone(),
        body,
    }))
}

fn empty_seen() -> seen::Seen {
    use crate::storage::header::Hex32;
    seen::Seen { generation: 0, manifest_hash: Hex32([0; 32]), state_commit: Hex32([0; 32]), recovery_auth: Vec::new() }
}
