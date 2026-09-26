//! Revisions from revoked authors (spec v0.4 §3.2, SY-11). When device D
//! is revoked, `Admit(D)` — the D-authored revisions the revoker had
//! accepted (revoker: its local DB at its revocation commit; everyone
//! else: those listed in the first accepted manifest whose registry
//! revokes D) — stays ordinary history. Every other D-authored revision is
//! refused: never admitted, deleted if held, counted.

use std::collections::HashSet;

use rusqlite::{params, Connection};

use super::rev_state;
use super::revision_rows::all_rows;
use super::revisions::{self, uuid_string, RevisionRow};
use crate::errors::ErrorCode;

fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    r.map_err(|_| ErrorCode::DbCorrupt)
}

pub fn is_revoked(conn: &Connection, author: &str) -> Result<bool, ErrorCode> {
    let n: i64 = db(conn.query_row("SELECT count(*) FROM revoked_authors WHERE author=?1", params![author], |r| r.get(0)))?;
    Ok(n > 0)
}

/// Whether `rev` must be refused: its author is revoked here and it is not
/// in `Admit(author)`.
pub fn refuses(conn: &Connection, rev: &RevisionRow) -> Result<bool, ErrorCode> {
    if !is_revoked(conn, &rev.author_device)? {
        return Ok(false);
    }
    let n: i64 = db(conn.query_row(
        "SELECT count(*) FROM admitted_by_revoked WHERE author=?1 AND revision_id=?2",
        params![rev.author_device, rev.revision_id.as_slice()],
        |r| r.get(0),
    ))?;
    Ok(n == 0)
}

/// Record D's revocation with `admit` as `Admit(D)` (first time only — the
/// set is fixed once), then drop every held D-authored revision outside
/// it and everything that descends from one. Returns the refused
/// revisions authored by `me` that must be re-authored.
pub fn record(conn: &Connection, device_id: &[u8; 16], admit: &HashSet<[u8; 32]>, me: &str) -> Result<Vec<RevisionRow>, ErrorCode> {
    let author = uuid_string(device_id);
    if is_revoked(conn, &author)? {
        return Ok(Vec::new());
    }
    db(conn.execute("INSERT INTO revoked_authors (author) VALUES (?1)", params![author]))?;
    for id in admit {
        db(conn.execute(
            "INSERT OR IGNORE INTO admitted_by_revoked (author, revision_id) VALUES (?1, ?2)",
            params![author, id.as_slice()],
        ))?;
    }
    purge(conn, me)
}

/// The revoker's side: `Admit(D)` is every D-authored revision it holds.
pub fn record_local(conn: &Connection, device_id: &[u8; 16], me: &str) -> Result<(), ErrorCode> {
    let author = uuid_string(device_id);
    let held: HashSet<[u8; 32]> = all_rows(conn)?.into_iter().filter(|r| r.author_device == author).map(|r| r.revision_id).collect();
    record(conn, device_id, &held, me).map(|_| ())
}

/// Remove refused revisions and their descendants from the admitted graph
/// and the pending table, count them, and recompute the affected records'
/// heads. Descendants authored by `me` are returned for re-authoring.
fn purge(conn: &Connection, me: &str) -> Result<Vec<RevisionRow>, ErrorCode> {
    let rows = all_rows(conn)?;
    let mut refused: HashSet<[u8; 32]> = HashSet::new();
    for r in &rows {
        if refuses(conn, r)? {
            refused.insert(r.revision_id);
        }
    }
    // Close over descendants (a revision with a refused ancestor can
    // never become applicable).
    loop {
        let before = refused.len();
        for r in &rows {
            if !refused.contains(&r.revision_id) && r.parent_ids.iter().any(|p| refused.contains(p)) {
                refused.insert(r.revision_id);
            }
        }
        if refused.len() == before {
            break;
        }
    }
    let mut mine = Vec::new();
    let mut records = HashSet::new();
    for r in rows.iter().filter(|r| refused.contains(&r.revision_id)) {
        db(conn.execute("DELETE FROM record_revs WHERE revision_id=?1", params![r.revision_id.as_slice()]))?;
        records.insert(r.record_id.clone());
        if r.author_device == me {
            mine.push(r.clone());
        } else {
            rev_state::count_refused(conn, &r.record_id, revisions::REFUSED_REVOKED_AUTHOR)?;
        }
    }
    for rid in records {
        super::merge::recompute_heads(conn, &rid)?;
    }
    rev_state::purge_pending(conn)?;
    Ok(mine)
}
