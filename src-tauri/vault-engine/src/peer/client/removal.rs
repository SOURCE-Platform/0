//! The revocation lock (spec v0.5 §22.9): once a verified `peer_status`
//! says the Mac removed this phone, the vault locks, makes no further peer
//! exchange and authors nothing. The marker persists across restarts and
//! is lifted only by a provider-confirmed state in which the phone is
//! still active (F.2d), or ended by "Remove this vault" (F.2d). Nothing is
//! deleted automatically.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::ErrorCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removal {
    /// The revocation is among the Mac's provider-committed entries.
    pub published: bool,
    /// Unix seconds when this phone learned of it.
    pub at: u64,
}

fn path(dir: &Path) -> PathBuf {
    dir.join("removal.json")
}

pub fn load(dir: &Path) -> Option<Removal> {
    std::fs::read(path(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

/// Written atomically; a later `published` never turns back to pending.
pub fn record(dir: &Path, published: bool, now: u64) -> Result<Removal, ErrorCode> {
    let r = match load(dir) {
        Some(old) => Removal { published: old.published || published, at: old.at },
        None => Removal { published, at: now },
    };
    let tmp = dir.join("removal.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&r).map_err(|_| ErrorCode::Internal)?).map_err(|_| ErrorCode::Internal)?;
    std::fs::rename(&tmp, path(dir)).map_err(|_| ErrorCode::Internal)?;
    Ok(r)
}
