//! Deterministic revision merge (spec v0.4 §3.2): the heads model, the
//! exact counter algorithm, tombstone conservatism, duplicate handling,
//! and pending revisions. No timestamp ever decides anything.
//!
//! **Heads are a function of the admitted graph alone** (SY-09): a
//! non-tombstone is a head iff it has no admitted child; a tombstone T is a
//! head unless some admitted revision names T as a *direct* parent among
//! two or more parents (a resolution, which honest devices author over all
//! current heads). A single-parent edit anywhere below T therefore never
//! resurrects the record, at any depth. Revisions are admitted only at the
//! local `vk_generation` (a copy at any other generation is never stored).
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
/// Held-back revisions per vault (SEC-O3): beyond this, more are refused.
pub const MAX_PENDING: u64 = 10_000;

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

fn reject(conn: &Connection, rev: &RevisionRow, reason: u8) -> Result<MergeOutcome, ErrorCode> {
    rev_state::count_refused(conn, &rev.record_id, reason)?;
    Ok(MergeOutcome::Rejected(reason))
}

/// Apply one revision under the §3.2 rules. `local_vk_generation` is this
/// vault's current generation: nothing sealed under another is admitted.
pub fn apply_revision(
    conn: &Connection,
    rev: &RevisionRow,
    local_vk_generation: u32,
    cmp: &dyn ContentCompare,
) -> Result<MergeOutcome, ErrorCode> {
    if rev.author_device == ZERO_AUTHOR || revisions::uuid_bytes(&rev.author_device).is_none() {
        return reject(conn, rev, revisions::REFUSED_ZERO_AUTHOR);
    }
    let malformed = !revisions::parents_canonical(&rev.parent_ids)
        || rev.parent_ids.contains(&rev.revision_id)
        || rev.counter > i64::MAX as u64
        || revisions::uuid_bytes(&rev.record_id).is_none();
    if malformed {
        return reject(conn, rev, revisions::REFUSED_MALFORMED);
    }
    if let Some(existing) = get_row(conn, &rev.revision_id)? {
        return duplicate(conn, &existing, rev, local_vk_generation, cmp);
    }
    if rev.vk_generation != local_vk_generation {
        return reject(conn, rev, revisions::REFUSED_GENERATION);
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
                if rev_state::pending_count(conn)? >= MAX_PENDING {
                    return reject(conn, rev, revisions::REFUSED_MALFORMED);
                }
                return rev_state::put_pending(conn, rev);
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
    revisions::insert_rev(conn, rev)?;
    let mut next = Vec::with_capacity(current.len() + 1);
    for h in current {
        // A tombstone head stays unless R covers it directly as a
        // resolution; every other ancestor of R stops being a head.
        let covered = if anc.contains(&h) {
            let tomb = get_row(conn, &h)?.is_some_and(|r| r.deleted);
            !tomb || (rev.parent_ids.len() >= 2 && rev.parent_ids.contains(&h))
        } else {
            false
        };
        if !covered {
            next.push(h);
        }
    }
    next.push(rev.revision_id);
    next.sort();
    set_heads(conn, &rev.record_id, &next)?;
    if !fork_evidence.is_empty() {
        fork_evidence.push(rev.revision_id);
        rev_state::freeze(conn, &rev.record_id, &fork_evidence)?;
    }
    Ok(MergeOutcome::Applied { conflicted: next.len() > 1 })
}

/// Same `revision_id` already admitted (§3.2 "Duplicates"). A copy at a
/// generation other than the local one is never stored (SEC-B1): only a
/// same-graph copy that brings a row *up to* the local generation replaces
/// it (the §2.10 re-seal adoption path).
fn duplicate(
    conn: &Connection,
    existing: &RevisionRow,
    rev: &RevisionRow,
    local_vk_generation: u32,
    cmp: &dyn ContentCompare,
) -> Result<MergeOutcome, ErrorCode> {
    if existing == rev {
        return Ok(MergeOutcome::AlreadyKnown);
    }
    if !existing.same_graph(rev) {
        freeze_both(conn, existing, rev)?;
        return Ok(MergeOutcome::Frozen);
    }
    if rev.vk_generation != local_vk_generation {
        return Ok(MergeOutcome::Superseded); // other generation: ignored
    }
    if existing.vk_generation < local_vk_generation {
        revisions::insert_rev(conn, rev)?; // brought up to the local generation
        return Ok(MergeOutcome::Superseded);
    }
    if cmp.same_content(existing, rev)? {
        // Benign: keep the lexicographically lower blob hash.
        let enc = |r: &RevisionRow| crate::backup::object::encode(r).map(|b| crate::backup::object::blob_hash(&b));
        if enc(rev)? < enc(existing)? {
            revisions::insert_rev(conn, rev)?;
        }
        return Ok(MergeOutcome::AlreadyKnown);
    }
    freeze_both(conn, existing, rev)?;
    Ok(MergeOutcome::Frozen)
}

/// Freeze the record the admitted copy belongs to (never only the one an
/// incoming forgery claims, SEC-O2).
fn freeze_both(conn: &Connection, existing: &RevisionRow, rev: &RevisionRow) -> Result<(), ErrorCode> {
    rev_state::freeze(conn, &existing.record_id, &[rev.revision_id])?;
    if rev.record_id != existing.record_id && revisions::uuid_bytes(&rev.record_id).is_some() {
        rev_state::freeze(conn, &rev.record_id, &[rev.revision_id])?;
    }
    Ok(())
}

/// Apply a batch in dependency order, then retry held revisions until no
/// further progress. Returns the outcome of every batch member.
pub fn apply_batch(
    conn: &Connection,
    rows: &[RevisionRow],
    local_vk_generation: u32,
    cmp: &dyn ContentCompare,
) -> Result<Vec<MergeOutcome>, ErrorCode> {
    let mut out = vec![MergeOutcome::Pending; rows.len()];
    for i in in_batch_order(rows) {
        out[i] = apply_revision(conn, &rows[i], local_vk_generation, cmp)?;
    }
    retry_pending(conn, local_vk_generation, cmp)?;
    Ok(out)
}

/// Re-apply held revisions until a full pass admits nothing new.
pub fn retry_pending(conn: &Connection, local_vk_generation: u32, cmp: &dyn ContentCompare) -> Result<(), ErrorCode> {
    loop {
        let before = rev_state::pending_count(conn)?;
        if before == 0 {
            return Ok(());
        }
        for obj in rev_state::take_pending(conn)? {
            let row = crate::backup::object::decode(&obj)?;
            apply_revision(conn, &row, local_vk_generation, cmp)?;
        }
        if rev_state::pending_count(conn)? >= before {
            return Ok(());
        }
    }
}

/// Parents-first order within the batch. Every row is scheduled exactly
/// once — including a second copy of an id, which then meets the duplicate
/// rule (SEC-I4); rows whose parents never become ready keep input order
/// and end up pending.
fn in_batch_order(rows: &[RevisionRow]) -> Vec<usize> {
    let ids: HashSet<[u8; 32]> = rows.iter().map(|r| r.revision_id).collect();
    let mut ready: HashSet<[u8; 32]> = HashSet::new();
    let mut scheduled = vec![false; rows.len()];
    let mut order = Vec::with_capacity(rows.len());
    loop {
        let before = order.len();
        for (i, r) in rows.iter().enumerate() {
            if !scheduled[i] && r.parent_ids.iter().all(|p| !ids.contains(p) || ready.contains(p)) {
                scheduled[i] = true;
                order.push(i);
                ready.insert(r.revision_id);
            }
        }
        if order.len() == rows.len() || order.len() == before {
            break;
        }
    }
    order.extend((0..rows.len()).filter(|i| !scheduled[*i]));
    order
}
