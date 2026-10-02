//! Spec v0.5 §22.7 on this Mac: the re-seal rule and the revocation
//! cutoff for peer deliveries.
//!
//! - **Set aside** (before every rotation and every adoption of a new key):
//!   a revision with no `own` or `provider` source is not carried into the
//!   new key. It leaves the graph (it returns only if it later arrives in a
//!   provider-confirmed state); this device's own revisions built on it —
//!   tombstones included — are re-authored onto their nearest remaining
//!   ancestors, same content, so the next published index is always
//!   ancestor-closed. All of it is one transaction and one flip (review
//!   SEC-B1 / VER-B4): a crash or a failed rotation afterwards leaves an
//!   ordinary, consistent vault.
//! - **Cut off** (at a revocation of D, by the revoker, and by every device
//!   that accepts D's revocation): a revision whose only source is D is
//!   refused for good (`refused_peer`, `PROVENANCE_REFUSED`), whatever
//!   author it claims. The revoker records it inside the rotation's staged
//!   database, so a rotation that fails refuses nothing.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection};

use super::merge::{apply_revision, MergeOutcome, NoCompare};
use super::revisions::{RevisionRow, MAX_PARENTS, REFUSED_PROVENANCE};
use super::sources::{self, Source};
use super::{rev_state, revision_rows, VaultStore};
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;

fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    r.map_err(|_| ErrorCode::DbCorrupt)
}

/// The revisions (id, record) whose only source is `peer`, except `keep`.
pub fn only_from(conn: &Connection, peer: &[u8; 16], keep: &HashSet<[u8; 32]>) -> Result<Vec<([u8; 32], String)>, ErrorCode> {
    let mut out = Vec::new();
    for r in revision_rows::all_rows(conn)? {
        if !keep.contains(&r.revision_id) && sources::only_from(conn, &r.revision_id, peer)? {
            out.push((r.revision_id, r.record_id));
        }
    }
    Ok(out)
}

/// Refuse `ids` for good and drop `peer`'s held puts (wire annex A.3.5).
/// Joins the caller's transaction.
pub fn refuse(conn: &Connection, peer: &[u8; 16], ids: &[([u8; 32], String)]) -> Result<(), ErrorCode> {
    db(conn.execute("DELETE FROM peer_inbox WHERE sender = ?1", params![&peer[..]]))?;
    for (id, record) in ids {
        if db(conn.execute("INSERT OR IGNORE INTO refused_peer (revision_id) VALUES (?1)", params![&id[..]]))? == 1 {
            rev_state::count_refused(conn, record, REFUSED_PROVENANCE)?;
        }
    }
    Ok(())
}

/// `only_from` + `refuse`: an accepted revocation of `peer`. Returns how
/// many revisions were refused.
pub fn cut_off(conn: &Connection, peer: &[u8; 16], keep: &HashSet<[u8; 32]>) -> Result<usize, ErrorCode> {
    let ids = only_from(conn, peer, keep)?;
    refuse(conn, peer, &ids)?;
    Ok(ids.len())
}

/// Whether a revision was cut off (checked before any admission).
pub fn refused(conn: &Connection, revision_id: &[u8; 32]) -> Result<bool, ErrorCode> {
    let n: i64 = db(conn.query_row("SELECT count(*) FROM refused_peer WHERE revision_id = ?1", params![&revision_id[..]], |r| r.get(0)))?;
    Ok(n > 0)
}

fn parents_first(rows: &[RevisionRow]) -> Vec<&RevisionRow> {
    let mut done: HashSet<[u8; 32]> = HashSet::new();
    let ids: HashSet<[u8; 32]> = rows.iter().map(|r| r.revision_id).collect();
    let mut out = Vec::with_capacity(rows.len());
    while out.len() < rows.len() {
        let before = out.len();
        for r in rows {
            if !done.contains(&r.revision_id) && r.parent_ids.iter().all(|p| !ids.contains(p) || done.contains(p)) {
                done.insert(r.revision_id);
                out.push(r);
            }
        }
        if out.len() == before {
            break; // a cycle cannot occur in a valid graph; stop rather than spin
        }
    }
    out
}

/// The revisions that leave the graph: no `own`/`provider` source (an
/// empty source set is unconfirmed too, review SEC-I2), cut off, or
/// resting on one of those.
fn leaving(conn: &Connection, rows: &[RevisionRow]) -> Result<HashSet<[u8; 32]>, ErrorCode> {
    let mut gone = HashSet::new();
    for r in rows {
        if !sources::confirmed(conn, &r.revision_id)? || refused(conn, &r.revision_id)? {
            gone.insert(r.revision_id);
        }
    }
    loop {
        let before = gone.len();
        for r in rows {
            if !gone.contains(&r.revision_id) && r.parent_ids.iter().any(|p| gone.contains(p)) {
                gone.insert(r.revision_id);
            }
        }
        if gone.len() == before {
            return Ok(gone);
        }
    }
}

/// §22.7 "parents = the nearest re-sealed ancestors": a remaining parent
/// stays; a re-authored one is replaced by its new id; any other leaving
/// parent is replaced by its own nearest ancestors (review VER-I6).
fn nearest(p: &[u8; 32], gone: &HashSet<[u8; 32]>, renamed: &HashMap<[u8; 32], [u8; 32]>, by_id: &HashMap<[u8; 32], &RevisionRow>, out: &mut Vec<[u8; 32]>) {
    if !gone.contains(p) {
        out.push(*p);
    } else if let Some(n) = renamed.get(p) {
        out.push(*n);
    } else if let Some(r) = by_id.get(p) {
        for q in &r.parent_ids {
            nearest(q, gone, renamed, by_id, out);
        }
    }
}

/// Before a rotation or an adoption under `vk`: drop unconfirmed and
/// cut-off revisions and anything resting on them; re-author this device's
/// own revisions among those. One transaction, then one flip. Returns how
/// many revisions left without a re-authored copy.
pub fn set_aside(store: &mut VaultStore, vk: &SecretBytes<32>) -> Result<usize, ErrorCode> {
    let rows = revision_rows::all_rows(&store.conn)?;
    let gone = leaving(&store.conn, &rows)?;
    if gone.is_empty() {
        return Ok(0);
    }
    let by_id: HashMap<[u8; 32], &RevisionRow> = rows.iter().map(|r| (r.revision_id, r)).collect();
    let mut mine: Vec<&RevisionRow> = Vec::new();
    for r in parents_first(&rows).into_iter().filter(|r| gone.contains(&r.revision_id)) {
        if sources::of(&store.conn, &r.revision_id)?.contains(&Source::Own) && !refused(&store.conn, &r.revision_id)? {
            mine.push(r);
        }
    }
    let gen = store.header.vk_generation;
    let target = store.flip_target(store.header.clone());
    {
        let tx = db(store.conn.unchecked_transaction())?;
        let mut records = HashSet::new();
        for id in &gone {
            db(tx.execute("DELETE FROM record_revs WHERE revision_id = ?1", params![&id[..]]))?;
            db(tx.execute("DELETE FROM rev_sources WHERE revision_id = ?1", params![&id[..]]))?;
            records.insert(by_id[id].record_id.clone());
        }
        for rid in &records {
            super::merge::recompute_heads(&tx, rid)?;
        }
        rev_state::purge_pending(&tx)?;
        let mut renamed: HashMap<[u8; 32], [u8; 32]> = HashMap::new();
        for old in &mine {
            let mut parents = Vec::new();
            for p in &old.parent_ids {
                nearest(p, &gone, &renamed, &by_id, &mut parents);
            }
            parents.sort();
            parents.dedup();
            parents.truncate(MAX_PARENTS);
            let rev = store.reauthored(vk, old, parents)?;
            match apply_revision(&tx, &rev, gen, &NoCompare)? {
                MergeOutcome::Applied { .. } => {}
                _ => return Err(ErrorCode::Internal), // dropped with the transaction
            }
            rev_state::bump_hwm(&tx, &rev.record_id, rev.counter)?;
            sources::add(&tx, &rev.revision_id, Source::Own)?;
            renamed.insert(old.revision_id, rev.revision_id);
        }
        super::flip::stamp(&tx, &target)?;
        db(tx.commit())?;
    }
    store.persist_head()?;
    Ok(gone.len() - mine.len())
}
