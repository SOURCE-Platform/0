//! What this Mac may serve (spec v0.5 §22.7, wire annex A.3.3/A.3.4):
//! provider-confirmed revisions always; local-only ones (no `provider`
//! source — unpublished own edits, peer deliveries) only when the Mac is
//! unlocked and verified its provider state in the last 15 minutes. Heads
//! are those of this **servable subgraph**, so `peer_hello`'s digest,
//! `peer_heads` and `peer_revs_get` always agree.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::Ctx;
use crate::errors::ErrorCode;
use crate::storage::revision_rows::all_rows;
use crate::storage::revisions::{uuid_bytes, RevisionRow};
use crate::storage::sources::{self, Source};
use crate::storage::VaultStore;

/// The servable revisions, grouped by record (16-byte ids, ascending),
/// plus the records that hold revisions but none servable (withheld).
pub struct Servable {
    pub records: BTreeMap<[u8; 16], Vec<RevisionRow>>,
    pub withheld: BTreeSet<[u8; 16]>,
}

pub fn servable(ctx: &Ctx, store: &VaultStore) -> Result<Servable, ErrorCode> {
    let local_ok = ctx.fresh && !ctx.locked;
    let mut records: BTreeMap<[u8; 16], Vec<RevisionRow>> = BTreeMap::new();
    let mut seen_records = BTreeSet::new();
    for row in all_rows(&store.conn)? {
        let rid = uuid_bytes(&row.record_id).ok_or(ErrorCode::DbCorrupt)?;
        seen_records.insert(rid);
        let confirmed = sources::of(&store.conn, &row.revision_id)?.contains(&Source::Provider);
        if confirmed || local_ok {
            records.entry(rid).or_default().push(row);
        }
    }
    let withheld = seen_records.into_iter().filter(|r| !records.contains_key(r)).collect();
    Ok(Servable { records, withheld })
}

/// Heads of one record's servable subgraph, ascending, by the store's own
/// rule (`merge::recompute_heads`, review VER-O2): a non-tombstone with no
/// child, or a tombstone no ≥2-parent revision names directly.
pub fn heads(rows: &[RevisionRow]) -> Vec<[u8; 32]> {
    let parents: HashSet<[u8; 32]> = rows.iter().flat_map(|r| r.parent_ids.iter().copied()).collect();
    let covering: HashSet<[u8; 32]> = rows.iter().filter(|r| r.parent_ids.len() >= 2).flat_map(|r| r.parent_ids.iter().copied()).collect();
    let mut h: Vec<[u8; 32]> = rows
        .iter()
        .filter(|r| if r.deleted { !covering.contains(&r.revision_id) } else { !parents.contains(&r.revision_id) })
        .map(|r| r.revision_id)
        .collect();
    h.sort();
    h
}

/// Every revision of `rows` that is an ancestor of (or equal to) one of
/// `known` — what the requester already holds.
fn held_by_requester(rows: &[RevisionRow], known: &[[u8; 32]]) -> HashSet<[u8; 32]> {
    let by_id: HashMap<[u8; 32], &RevisionRow> = rows.iter().map(|r| (r.revision_id, r)).collect();
    let mut out = HashSet::new();
    let mut stack: Vec<[u8; 32]> = known.iter().copied().filter(|k| by_id.contains_key(k)).collect();
    while let Some(id) = stack.pop() {
        if out.insert(id) {
            if let Some(r) = by_id.get(&id) {
                stack.extend(r.parent_ids.iter().copied());
            }
        }
    }
    out
}

/// The closure to send for one record, in the canonical order (wire
/// annex A.3.4): Kahn's algorithm always emitting the smallest ready
/// `revision_id`. Parents the requester holds count as satisfied.
pub fn closure(rows: &[RevisionRow], requester_heads: &[[u8; 32]]) -> Vec<RevisionRow> {
    let held = held_by_requester(rows, requester_heads);
    let send: Vec<&RevisionRow> = rows.iter().filter(|r| !held.contains(&r.revision_id)).collect();
    let ids: HashSet<[u8; 32]> = send.iter().map(|r| r.revision_id).collect();
    let mut waiting: HashMap<[u8; 32], usize> = HashMap::new();
    let mut children: HashMap<[u8; 32], Vec<[u8; 32]>> = HashMap::new();
    for r in &send {
        let inner: Vec<&[u8; 32]> = r.parent_ids.iter().filter(|p| ids.contains(*p)).collect();
        waiting.insert(r.revision_id, inner.len());
        for p in inner {
            children.entry(*p).or_default().push(r.revision_id);
        }
    }
    let by_id: HashMap<[u8; 32], &RevisionRow> = send.iter().map(|r| (r.revision_id, *r)).collect();
    let mut ready: BTreeSet<[u8; 32]> = waiting.iter().filter(|(_, n)| **n == 0).map(|(id, _)| *id).collect();
    let mut out = Vec::with_capacity(send.len());
    while let Some(id) = ready.pop_first() {
        out.push((*by_id[&id]).clone());
        for c in children.get(&id).into_iter().flatten() {
            let n = waiting.get_mut(c).expect("child of a sent revision");
            *n -= 1;
            if *n == 0 {
                ready.insert(*c);
            }
        }
    }
    out
}
