//! Spec v0.5 §22.7 on this Mac: the re-seal rule and the revocation
//! cutoff for peer deliveries.
//!
//! - **Set aside** (before every rotation and every adoption of a new key):
//!   a revision whose only sources are peers is not carried into the new
//!   key. It leaves the graph (it returns only if it later arrives in a
//!   provider-confirmed state); this device's own revisions built on it
//!   are re-authored onto the remaining heads, same content — so the next
//!   published index is always ancestor-closed.
//! - **Cut off** (at a revocation of D, by the revoker, and by every device
//!   that accepts D's revocation): a revision whose only source is D is
//!   refused for good (`refused_peer`), whatever author it claims.

use std::collections::HashSet;

use rusqlite::params;

use super::revisions::{RevisionRow, REFUSED_REVOKED_AUTHOR};
use super::sources::{self, Source};
use super::{rev_state, revision_rows, VaultStore};
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;

fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    r.map_err(|_| ErrorCode::DbCorrupt)
}

/// Refuse every revision whose only source is `peer`, except `keep` (the
/// ids its revocation's provider index lists). Returns how many.
pub fn cut_off(store: &VaultStore, peer: &[u8; 16], keep: &HashSet<[u8; 32]>) -> Result<usize, ErrorCode> {
    // Its held puts go too (wire annex A.3.5).
    db(store.conn.execute("DELETE FROM peer_inbox WHERE sender = ?1", params![&peer[..]]))?;
    let mut n = 0;
    for r in revision_rows::all_rows(&store.conn)? {
        if !keep.contains(&r.revision_id) && sources::only_from(&store.conn, &r.revision_id, peer)? {
            db(store.conn.execute("INSERT OR IGNORE INTO refused_peer (revision_id) VALUES (?1)", params![&r.revision_id[..]]))?;
            rev_state::count_refused(&store.conn, &r.record_id, REFUSED_REVOKED_AUTHOR)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Whether a revision was cut off (checked before any admission).
pub fn refused(conn: &rusqlite::Connection, revision_id: &[u8; 32]) -> Result<bool, ErrorCode> {
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

/// Before a rotation or an adoption under `vk`: drop peer-only and
/// cut-off revisions and anything resting on them; re-author this
/// device's own revisions among those. Returns how many were set aside.
pub fn set_aside(store: &mut VaultStore, vk: &SecretBytes<32>) -> Result<usize, ErrorCode> {
    let rows = revision_rows::all_rows(&store.conn)?;
    let mut gone: HashSet<[u8; 32]> = HashSet::new();
    for r in &rows {
        let s = sources::of(&store.conn, &r.revision_id)?;
        let peer_only = !s.is_empty() && !s.iter().any(|x| matches!(x, Source::Own | Source::Provider));
        if peer_only || refused(&store.conn, &r.revision_id)? {
            gone.insert(r.revision_id);
        }
    }
    if gone.is_empty() {
        return Ok(0);
    }
    loop {
        let before = gone.len();
        for r in &rows {
            if !gone.contains(&r.revision_id) && r.parent_ids.iter().any(|p| gone.contains(p)) {
                gone.insert(r.revision_id);
            }
        }
        if gone.len() == before {
            break;
        }
    }
    let mut mine: Vec<RevisionRow> = Vec::new();
    let mut records = HashSet::new();
    let own = |id: &[u8; 32]| sources::of(&store.conn, id).map(|s| s.contains(&Source::Own));
    // Own revisions are re-authored in dependency order (parents first).
    for r in parents_first(&rows).into_iter().filter(|r| gone.contains(&r.revision_id)) {
        if own(&r.revision_id)? && !refused(&store.conn, &r.revision_id)? {
            mine.push(r.clone());
        }
        db(store.conn.execute("DELETE FROM record_revs WHERE revision_id = ?1", params![&r.revision_id[..]]))?;
        db(store.conn.execute("DELETE FROM rev_sources WHERE revision_id = ?1", params![&r.revision_id[..]]))?;
        records.insert(r.record_id.clone());
    }
    for rid in &records {
        super::merge::recompute_heads(&store.conn, rid)?;
    }
    rev_state::purge_pending(&store.conn)?;
    let set_aside = gone.len() - mine.len();
    for r in mine.iter().filter(|r| !r.deleted) {
        store.reauthor(vk, r)?;
    }
    Ok(set_aside)
}
