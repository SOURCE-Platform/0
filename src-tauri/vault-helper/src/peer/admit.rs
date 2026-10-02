//! `peer_revs_put` on an unlocked Mac (spec v0.5 §22.7, wire annex
//! A.3.5): a batch must be in the canonical order with its full parent
//! closure (else the whole batch is `FORMAT_INVALID`); a revision at
//! another key generation or by an author this registry does not know
//! waits (not stored, not counted as refused — the phone sends it again);
//! every other one is AEAD-opened under the current key **before**
//! admission (a forged tombstone never enters); admitted revisions record
//! the sending peer as their source.

use std::collections::HashSet;

use vault_proto::backup::object;
use vault_proto::peer::body::PutCounts;
use vault_proto::peer::exchange::Revs;

use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::registry::chain::EpochPolicy;
use crate::storage::merge::{apply_batch, MergeOutcome};
use crate::storage::revisions::{get_row, uuid_bytes, RevisionRow, REFUSED_MALFORMED};
use crate::storage::sources::{self, Source};
use crate::storage::{rev_state, VaultStore};
use crate::sync::compare::VkCompare;

pub const MAX_BATCH: usize = 2_000;

/// `Err(FormatInvalid)` → the caller answers status 4, nothing applied.
pub fn put(store: &mut VaultStore, vk: &SecretBytes<32>, sender: [u8; 16], body: &[u8]) -> Result<PutCounts, ErrorCode> {
    let batch = Revs::decode(body, true)?;
    if batch.objects.len() > MAX_BATCH {
        return Err(ErrorCode::FormatInvalid);
    }
    let rows: Vec<RevisionRow> = batch.objects.iter().map(|o| object::decode(o)).collect::<Result<_, _>>().map_err(|_| ErrorCode::FormatInvalid)?;
    canonical(&rows)?;
    closed(store, &rows)?;
    let reg = crate::registry::log::read_state(&store.dir, &store.header.vault_id.0, &EpochPolicy::CheckpointAnchored)?;
    let gen = store.header.vk_generation;
    let mut counts = PutCounts { admitted: 0, waiting: 0, refused: 0 };
    let mut admissible = Vec::new();
    for row in rows {
        let known_author = uuid_bytes(&row.author_device).is_some_and(|a| reg.devices.iter().any(|d| d.device_id == a));
        if row.vk_generation != gen || !known_author {
            counts.waiting += 1;
            continue;
        }
        if store.open_row(vk, &row).is_err() || store.open_row_meta(vk, &row).is_err() {
            rev_state::count_refused(&store.conn, &row.record_id, REFUSED_MALFORMED)?;
            counts.refused += 1;
            continue;
        }
        admissible.push(row);
    }
    let target = store.flip_target(store.header.clone());
    {
        let tx = store.conn.unchecked_transaction().map_err(|_| ErrorCode::DbCorrupt)?;
        let outcomes = apply_batch(&tx, &admissible, gen, &VkCompare { store, vk })?;
        for (row, o) in admissible.iter().zip(&outcomes) {
            match o {
                MergeOutcome::Rejected(_) => counts.refused += 1,
                MergeOutcome::Pending => counts.waiting += 1,
                _ => {
                    counts.admitted += 1;
                    sources::add(&tx, &row.revision_id, Source::Peer(sender))?;
                }
            }
        }
        crate::storage::flip::stamp(&tx, &target)?;
        tx.commit().map_err(|_| ErrorCode::DbCorrupt)?;
    }
    store.persist_head()?;
    Ok(counts)
}

/// Grouped by record in ascending `record_id`, each record in the
/// canonical Kahn order (smallest ready `revision_id` first).
fn canonical(rows: &[RevisionRow]) -> Result<(), ErrorCode> {
    let mut i = 0;
    let mut last: Option<[u8; 16]> = None;
    while i < rows.len() {
        let rid = uuid_bytes(&rows[i].record_id).ok_or(ErrorCode::FormatInvalid)?;
        if last.is_some_and(|l| l >= rid) {
            return Err(ErrorCode::FormatInvalid);
        }
        let j = i + rows[i..].iter().take_while(|r| r.record_id == rows[i].record_id).count();
        let expected = super::graph::closure(&rows[i..j], &[]);
        if expected.iter().map(|r| r.revision_id).ne(rows[i..j].iter().map(|r| r.revision_id)) {
            return Err(ErrorCode::FormatInvalid);
        }
        last = Some(rid);
        i = j;
    }
    Ok(())
}

/// Every parent is in the batch or already held here.
fn closed(store: &VaultStore, rows: &[RevisionRow]) -> Result<(), ErrorCode> {
    let ids: HashSet<[u8; 32]> = rows.iter().map(|r| r.revision_id).collect();
    for r in rows {
        for p in &r.parent_ids {
            if !ids.contains(p) && get_row(&store.conn, p)?.is_none() {
                return Err(ErrorCode::FormatInvalid);
            }
        }
    }
    Ok(())
}
