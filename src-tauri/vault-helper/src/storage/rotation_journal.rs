//! Crash-safe commit for VK rotation (spec §2.10).
//!
//! Rotation stages the complete rotated vault beside the live one
//! (`vault.db.next`, `wraps/*.next`, `header.json.next`,
//! `manifest.json.next`), then writes ONE commit marker
//! (`rotation.commit`, atomic rename) — the commit point — and only then
//! renames the staged files over the live ones.
//!
//! `recover_pending` runs before every open:
//! - marker present → roll forward (finish every rename, idempotent);
//! - staged files without a marker → roll back (delete them).
//!
//! So an opened vault is always entirely pre-rotation or entirely
//! post-rotation; a half-rotated state is never accepted. (The spec's
//! wording "re-runs rotation from scratch" is replaced by this
//! roll-forward/roll-back journal: re-running would need the new VK,
//! which a crash would otherwise lose. Documented Phase D deviation.)

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::db::DB_NAME;
use super::manifest::MANIFEST_NAME;
use super::store::{write_atomic, PASSWORD_WRAP_NAME, RECOVERY_WRAP_NAME};
use crate::errors::ErrorCode;
use crate::VAULT_HEADER_NAME;

pub const COMMIT_MARKER: &str = "rotation.commit";
const NEXT_SUFFIX: &str = "next";

/// Files a rotation may replace, in roll-forward order. The DB goes
/// first; header + manifest (the "accepted" pair) go last.
pub const STAGED: [&str; 5] = [
    DB_NAME,
    PASSWORD_WRAP_NAME,
    RECOVERY_WRAP_NAME,
    VAULT_HEADER_NAME,
    MANIFEST_NAME,
];

#[derive(Serialize, Deserialize)]
pub struct CommitMarker {
    pub new_vk_generation: u32,
    pub new_manifest_generation: u64,
    /// Live files the rotation deletes (e.g. a recovery.wrap that could
    /// not be re-sealed because the caller did not hold the RK).
    pub remove: Vec<String>,
    /// Staged files beyond `STAGED`: the per-device envelopes and the
    /// device-credential store, whose names depend on which devices are
    /// enrolled (§11.4). Rolled forward in the same pass, so a rotation
    /// can never commit the new VK while leaving envelopes on the old one.
    #[serde(default)]
    pub stage: Vec<String>,
}

pub fn next_path(dir: &Path, name: &str) -> PathBuf {
    let live = dir.join(name);
    let file = live.file_name().expect("file name").to_string_lossy().to_string();
    live.with_file_name(format!("{file}.{NEXT_SUFFIX}"))
}

/// Durably write the commit marker. After this returns, the rotation is
/// committed: any later crash rolls forward.
pub fn write_marker(dir: &Path, marker: &CommitMarker) -> Result<(), ErrorCode> {
    let bytes = serde_json::to_vec(marker).map_err(|_| ErrorCode::Internal)?;
    write_atomic(&dir.join(COMMIT_MARKER), &bytes)
}

/// Delete every staged file (rotation abandoned before its commit point).
pub fn discard_staged(dir: &Path) {
    for name in STAGED {
        let _ = std::fs::remove_file(next_path(dir, name));
    }
    // Device envelopes are staged under names that depend on which
    // devices are enrolled; sweep the directory rather than a fixed list.
    if let Ok(entries) = std::fs::read_dir(dir.join("wraps").join("devices")) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().ends_with(".next") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let _ = std::fs::remove_file(next_path(dir, DB_NAME).with_extension("next-journal"));
}

/// Finish a committed rotation. The live DB was checkpointed and closed
/// before the marker was written, so its WAL side files hold nothing and
/// must go before the new DB takes its name (a stale WAL would otherwise
/// be replayed onto the new file).
fn roll_forward(dir: &Path, marker: &CommitMarker) -> Result<(), ErrorCode> {
    for name in STAGED.iter().copied().chain(marker.stage.iter().map(String::as_str)) {
        let next = next_path(dir, name);
        if !next.exists() {
            continue; // already renamed by an earlier, interrupted pass
        }
        if name == DB_NAME {
            for side in ["-wal", "-shm"] {
                let _ = std::fs::remove_file(dir.join(format!("{DB_NAME}{side}")));
            }
        }
        std::fs::rename(&next, dir.join(name)).map_err(|_| ErrorCode::Internal)?;
    }
    for name in &marker.remove {
        let _ = std::fs::remove_file(dir.join(name));
    }
    fsync_dir(dir);
    std::fs::remove_file(dir.join(COMMIT_MARKER)).map_err(|_| ErrorCode::Internal)?;
    fsync_dir(dir);
    Ok(())
}

fn fsync_dir(dir: &Path) {
    for d in [dir.to_path_buf(), dir.join("wraps")] {
        if let Ok(f) = std::fs::File::open(&d) {
            let _ = f.sync_all();
        }
    }
}

/// Resolve any interrupted rotation before the vault is opened.
pub fn recover_pending(dir: &Path) -> Result<(), ErrorCode> {
    let marker_path = dir.join(COMMIT_MARKER);
    match std::fs::read(&marker_path) {
        Ok(bytes) => {
            let marker: CommitMarker =
                serde_json::from_slice(&bytes).map_err(|_| ErrorCode::DbCorrupt)?;
            roll_forward(dir, &marker)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            discard_staged(dir);
            Ok(())
        }
        Err(_) => Err(ErrorCode::DbCorrupt),
    }
}

/// Test-only fault injection: stop the rotation right after `FailAt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailAt {
    AfterDbStaged,
    AfterWrapsStaged,
    AfterHeadStaged,
    AfterMarker,
    AfterFirstRename,
}

/// Commit a fully staged rotation: marker, then renames. `fail` stops
/// early to simulate a crash (the vault must then open cleanly).
pub fn commit(dir: &Path, marker: &CommitMarker, fail: Option<FailAt>) -> Result<(), ErrorCode> {
    if fail == Some(FailAt::AfterHeadStaged) {
        return Err(ErrorCode::Internal);
    }
    write_marker(dir, marker)?;
    if fail == Some(FailAt::AfterMarker) {
        return Err(ErrorCode::Internal);
    }
    if fail == Some(FailAt::AfterFirstRename) {
        let next = next_path(dir, DB_NAME);
        for side in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(dir.join(format!("{DB_NAME}{side}")));
        }
        std::fs::rename(next, dir.join(DB_NAME)).map_err(|_| ErrorCode::Internal)?;
        return Err(ErrorCode::Internal);
    }
    roll_forward(dir, marker)
}
