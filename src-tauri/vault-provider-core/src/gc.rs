//! Provider-controlled retention GC (spec v0.4 §11.2, owner decision).
//! Reachable: the current and two retained states (their manifest,
//! checkpoint and index blobs), every blob those indexes list, and every
//! blob younger than 7 days. Everything else is deleted. A commit that
//! races GC fails with `412 BLOB_MISSING` and the client re-uploads.

use std::collections::HashSet;

use vault_proto::backup::index::ObjectIndex;
use vault_proto::errors::ErrorCode;

use crate::Provider;

pub const MIN_AGE: u64 = 7 * 24 * 3600;

impl Provider {
    /// Collect one vault; returns the number of blobs deleted.
    pub fn gc_vault(&self, vid: &[u8; 16], now: u64) -> Result<usize, ErrorCode> {
        let Some((state, _)) = self.load_state(vid).map_err(|r| r.0)? else {
            return Ok(0);
        };
        let mut keep: HashSet<[u8; 32]> = HashSet::new();
        for r in std::iter::once(state.current_ref()).chain(state.retained.iter().cloned()) {
            keep.extend([r.manifest_hash.0, r.checkpoint_hash.0, r.index_hash.0]);
            let bytes = self.blobs.get(vid, &r.index_hash.0).map_err(|_| ErrorCode::BackupUnavailable)?;
            // An unreadable retained index keeps nothing extra alive but
            // never aborts GC of the rest; the current one must parse.
            match bytes.map(|b| ObjectIndex::decode(&b)) {
                Some(Ok(idx)) => keep.extend(idx.blobs()),
                _ if r.generation == state.generation => return Err(ErrorCode::IndexInvalid),
                _ => {}
            }
        }
        let mut deleted = 0;
        for (sha, created) in self.blobs.list(vid).map_err(|_| ErrorCode::BackupUnavailable)? {
            if !keep.contains(&sha) && now.saturating_sub(created) >= MIN_AGE {
                self.blobs.delete(vid, &sha).map_err(|_| ErrorCode::BackupUnavailable)?;
                deleted += 1;
            }
        }
        Ok(deleted)
    }
}
