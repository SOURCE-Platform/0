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

fn tmp(dir: &Path) -> PathBuf {
    dir.join("removal.json.tmp")
}

/// Fails closed (review SEC-I3): the marker's *existence* means removed —
/// a torn, empty or unreadable file still locks (read as pending), and so
/// does a write interrupted before its rename.
pub fn load(dir: &Path) -> Option<Removal> {
    let fallback = Removal { published: false, at: 0 };
    for p in [path(dir), tmp(dir)] {
        match std::fs::read(&p) {
            Ok(b) => return Some(serde_json::from_slice(&b).unwrap_or(fallback)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Some(fallback),
        }
    }
    None
}

/// Written and synced before the rename, and the directory synced after
/// it; a later `published` never turns back to pending.
pub fn record(dir: &Path, published: bool, now: u64) -> Result<Removal, ErrorCode> {
    use std::io::Write;
    let r = match load(dir) {
        Some(old) => Removal { published: old.published || published, at: if old.at == 0 { now } else { old.at } },
        None => Removal { published, at: now },
    };
    let bytes = serde_json::to_vec(&r).map_err(|_| ErrorCode::Internal)?;
    let mut f = std::fs::File::create(tmp(dir)).map_err(|_| ErrorCode::Internal)?;
    f.write_all(&bytes).and_then(|_| f.sync_all()).map_err(|_| ErrorCode::Internal)?;
    std::fs::rename(tmp(dir), path(dir)).map_err(|_| ErrorCode::Internal)?;
    std::fs::File::open(dir).and_then(|d| d.sync_all()).map_err(|_| ErrorCode::Internal)?;
    Ok(r)
}

/// The lock as it applies here: only in SOURCE Vault's engine (review
/// SEC-O1 and its re-review — a flag set at boot, never a file the same
/// user could edit), so a marker in the Mac helper's directory is inert.
pub fn active(phone: bool, dir: &Path) -> Option<Removal> {
    if phone { load(dir) } else { None }
}
