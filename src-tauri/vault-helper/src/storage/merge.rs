//! Deterministic revision merge (spec v0.4 §3.2): the heads model, the
//! exact counter algorithm, tombstone conservatism, duplicate handling,
//! and pending revisions. No timestamp ever decides anything.
//!
//! Callers supply a transaction. Refusal of revoked authors (§3.2) is the
//! sync layer's decision and happens before `apply_revision`.

use std::collections::HashSet;

use rusqlite::{params, Connection};

use super::rev_state;
use super::revisions::{self, get_row, heads, set_heads, RevisionRow};
use crate::errors::ErrorCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeOutcome {
    /// Admitted; `conflicted` = the record now has more than one head.
    Applied { conflicted: bool },
    /// Exact duplicate of an admitted revision — no-op.
    AlreadyKnown,
    /// Another representation of an admitted revision (a rotation re-seal):
    /// the newer `vk_generation` is kept.
    Superseded,
    /// Parents not all admitted yet; held in `pending_revs`.
    Pending,
    /// Rejected and counted (reason code, `revisions::REFUSED_*`).
    Rejected(u8),
    /// Same id, conflicting content: the record is frozen with evidence.
    Frozen,
}

/// Decides whether two same-id, same-generation representations carry the
/// same plaintext. Only a VK holder can answer; without one, differing
/// bytes are treated as different content (fail toward freezing).
pub trait ContentCompare {
    fn same_content(&self, a: &RevisionRow, b: &RevisionRow) -> Result<bool, ErrorCode>;
}

pub struct NoCompare;

impl ContentCompare for NoCompare {
    fn same_content(&self, _a: &RevisionRow, _b: &RevisionRow) -> Result<bool, ErrorCode> {
        Ok(false)
    }
}

const ZERO_AUTHOR: &str = "00000000-0000-0000-0000-000000000000";

/// All admitted ancestors of a revision with the given parents.
pub fn ancestors(conn: &Connection, parents: &[[u8; 32]]) -> Result<HashSet<[u8; 32]>, ErrorCode> {
    let mut seen = HashSet::new();
    let mut stack: Vec<[u8; 32]> = parents.to_vec();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(row) = get_row(conn, &id)? {
            stack.extend(row.parent_ids.iter().copied());
        }
    }
    Ok(seen)
}

fn author_revisions(conn: &Connection, rev: &RevisionRow) -> Result<Vec<([u8; 32], u64)>, ErrorCode> {
    let mut stmt = conn
        .prepare("SELECT revision_id, counter FROM record_revs WHERE record_id=?1 AND author_device=?2")
        .map_err(|_| ErrorCode::DbCorrupt)?;
    let rows = stmt
        .query_map(params![rev.record_id, rev.author_device], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)? as u64))
        })
        .map_err(|_| ErrorCode::DbCorrupt)?;
    rows.map(|r| {
        let (id, c) = r.map_err(|_| ErrorCode::DbCorrupt)?;
        Ok((id.as_slice().try_into().map_err(|_| ErrorCode::DbCorrupt)?, c))
    })
    .collect()
}

/// Apply one revision under the §3.2 rules. `object` is its serialized
/// §3.7 form, kept if the revision has to wait in `pending_revs`.
pub fn apply_revision(
    conn: &Connection,
    rev: &RevisionRow,
    object: &[u8],
    cmp: &dyn ContentCompare,
) -> Result<MergeOutcome, ErrorCode> {
    if rev.author_device == ZERO_AUTHOR || revisions::uuid_bytes(&rev.author_device).is_none() {
        rev_state::count_refused(conn, &rev.record_id, revisions::REFUSED_ZERO_AUTHOR)?;
        return Ok(MergeOutcome::Rejected(revisions::REFUSED_ZERO_AUTHOR));
    }
    if !revisions::parents_canonical(&rev.parent_ids) || rev.parent_ids.contains(&rev.revision_id) {
        rev_state::count_refused(conn, &rev.record_id, revisions::REFUSED_MALFORMED)?;
        return Ok(MergeOutcome::Rejected(revisions::REFUSED_MALFORMED));
    }
    if let Some(existing) = get_row(conn, &rev.revision_id)? {
        return duplicate(conn, &existing, rev, cmp);
    }
    // Applicable only when every parent is admitted, in the same record.
    for p in &rev.parent_ids {
        match get_row(conn, p)? {
            Some(parent) if parent.record_id == rev.record_id => {}
            Some(_) => {
                rev_state::count_refused(conn, &rev.record_id, revisions::REFUSED_MALFORMED)?;
                return Ok(MergeOutcome::Rejected(revisions::REFUSED_MALFORMED));
            }
            None => {
                rev_state::put_pending(conn, rev, object)?;
                return Ok(MergeOutcome::Pending);
            }
        }
    }
    let anc = ancestors(conn, &rev.parent_ids)?;
    // Counter algorithm (§3.2 table), against every admitted P by the same
    // author on this record.
    let mut fork_evidence = Vec::new();
    for (p, counter) in author_revisions(conn, rev)? {
        if anc.contains(&p) {
            if counter >= rev.counter {
                rev_state::count_refused(conn, &rev.record_id, revisions::REFUSED_COUNTER_REGRESSION)?;
                return Ok(MergeOutcome::Rejected(revisions::REFUSED_COUNTER_REGRESSION));
            }
        } else {
            // Not an ancestor: equal counter = equivocation, any other =
            // author fork. Both are kept; the record freezes.
            fork_evidence.push(p);
        }
    }
    let current = heads(conn, &rev.record_id)?;
    // A resolution covers a genuine multi-head conflict; it is the one
    // revision allowed to keep an edit over a competing tombstone.
    let resolves = current.len() >= 2 && current.iter().all(|h| rev.parent_ids.contains(h));
    revisions::insert_rev(conn, rev)?;
    let mut next: Vec<[u8; 32]> = current.into_iter().filter(|h| !anc.contains(h)).collect();
    next.push(rev.revision_id);
    // Tombstone conservatism: an edit whose parent is a tombstone never
    // resurrects — the tombstone stays a head beside it (conflict).
    if !rev.deleted && !resolves {
        for p in &rev.parent_ids {
            if get_row(conn, p)?.is_some_and(|r| r.deleted) && !next.contains(p) {
                next.push(*p);
            }
        }
    }
    next.sort();
    set_heads(conn, &rev.record_id, &next)?;
    if !fork_evidence.is_empty() {
        fork_evidence.push(rev.revision_id);
        rev_state::freeze(conn, &rev.record_id, &fork_evidence)?;
    }
    Ok(MergeOutcome::Applied { conflicted: next.len() > 1 })
}

/// Same `revision_id` already admitted (§3.2 "Duplicates").
fn duplicate(
    conn: &Connection,
    existing: &RevisionRow,
    rev: &RevisionRow,
    cmp: &dyn ContentCompare,
) -> Result<MergeOutcome, ErrorCode> {
    if existing == rev {
        return Ok(MergeOutcome::AlreadyKnown);
    }
    if !existing.same_graph(rev) {
        rev_state::freeze(conn, &rev.record_id, &[rev.revision_id])?;
        return Ok(MergeOutcome::Frozen);
    }
    if rev.vk_generation > existing.vk_generation {
        revisions::insert_rev(conn, rev)?; // newer representation replaces
        return Ok(MergeOutcome::Superseded);
    }
    if rev.vk_generation < existing.vk_generation {
        return Ok(MergeOutcome::Superseded); // stale representation ignored
    }
    if cmp.same_content(existing, rev)? {
        // Benign: keep the lexicographically lower blob hash.
        let enc = |r: &RevisionRow| crate::backup::object::encode(r).map(|b| crate::backup::object::blob_hash(&b));
        if enc(rev)? < enc(existing)? {
            revisions::insert_rev(conn, rev)?;
        }
        return Ok(MergeOutcome::AlreadyKnown);
    }
    rev_state::freeze(conn, &rev.record_id, &[rev.revision_id])?;
    Ok(MergeOutcome::Frozen)
}

/// Apply a batch in dependency order, then retry held revisions until no
/// further progress. Returns the outcome of every batch member.
pub fn apply_batch(
    conn: &Connection,
    rows: &[RevisionRow],
    cmp: &dyn ContentCompare,
) -> Result<Vec<MergeOutcome>, ErrorCode> {
    let mut out = vec![MergeOutcome::Pending; rows.len()];
    for i in in_batch_order(rows) {
        let obj = crate::backup::object::encode(&rows[i])?;
        out[i] = apply_revision(conn, &rows[i], &obj, cmp)?;
    }
    retry_pending(conn, cmp)?;
    Ok(out)
}

/// Re-apply held revisions until a full pass admits nothing new.
pub fn retry_pending(conn: &Connection, cmp: &dyn ContentCompare) -> Result<(), ErrorCode> {
    loop {
        let before = rev_state::pending_count(conn)?;
        if before == 0 {
            return Ok(());
        }
        for obj in rev_state::take_pending(conn)? {
            let row = crate::backup::object::decode(&obj)?;
            apply_revision(conn, &row, &obj, cmp)?;
        }
        if rev_state::pending_count(conn)? >= before {
            return Ok(());
        }
    }
}

/// Parents-first order within the batch (members whose parents are
/// outside the batch come first; cycles fall back to input order and end
/// up pending).
fn in_batch_order(rows: &[RevisionRow]) -> Vec<usize> {
    let ids: HashSet<[u8; 32]> = rows.iter().map(|r| r.revision_id).collect();
    let mut done: HashSet<[u8; 32]> = HashSet::new();
    let mut order = Vec::with_capacity(rows.len());
    while order.len() < rows.len() {
        let before = order.len();
        for (i, r) in rows.iter().enumerate() {
            if !done.contains(&r.revision_id)
                && r.parent_ids.iter().all(|p| !ids.contains(p) || done.contains(p))
            {
                done.insert(r.revision_id);
                order.push(i);
            }
        }
        if order.len() == before {
            order.extend((0..rows.len()).filter(|i| !done.contains(&rows[*i].revision_id)));
            break;
        }
    }
    order
}
