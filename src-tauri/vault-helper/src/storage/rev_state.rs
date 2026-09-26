//! Per-record bookkeeping around the revision graph (spec v0.4 §3.2):
//! freeze flags, refusal counts, the author high-water mark, and the
//! pending table for revisions whose parents have not arrived yet.

use rusqlite::{params, Connection, OptionalExtension};

use super::revisions::RevisionRow;
use crate::errors::ErrorCode;

fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    r.map_err(|_| ErrorCode::DbCorrupt)
}

/// Freeze a record (equivocation / author fork): excluded from fills and
/// edits until an acknowledged `resolve_conflict`. Evidence ids append.
pub fn freeze(conn: &Connection, record_id: &str, evidence: &[[u8; 32]]) -> Result<(), ErrorCode> {
    let prior: Option<Vec<u8>> = db(conn
        .query_row("SELECT evidence FROM record_flags WHERE record_id=?1", params![record_id], |r| r.get(0))
        .optional())?;
    let mut ev = prior.unwrap_or_default();
    for id in evidence {
        if !ev.chunks(32).any(|c| c == id) {
            ev.extend_from_slice(id);
        }
    }
    db(conn.execute(
        "INSERT INTO record_flags (record_id, frozen, evidence) VALUES (?1, 1, ?2)
         ON CONFLICT(record_id) DO UPDATE SET frozen=1, evidence=excluded.evidence",
        params![record_id, ev],
    ))?;
    Ok(())
}

pub fn is_frozen(conn: &Connection, record_id: &str) -> Result<bool, ErrorCode> {
    let f: Option<i64> = db(conn
        .query_row("SELECT frozen FROM record_flags WHERE record_id=?1", params![record_id], |r| r.get(0))
        .optional())?;
    Ok(f == Some(1))
}

pub fn unfreeze(conn: &Connection, record_id: &str) -> Result<(), ErrorCode> {
    db(conn.execute("DELETE FROM record_flags WHERE record_id=?1", params![record_id]))?;
    Ok(())
}

/// Count one refused/rejected revision for a record (counts only, §3.2).
pub fn count_refused(conn: &Connection, record_id: &str, reason: u8) -> Result<(), ErrorCode> {
    db(conn.execute(
        "INSERT INTO refused_revs (record_id, reason, count) VALUES (?1, ?2, 1)
         ON CONFLICT(record_id, reason) DO UPDATE SET count = count + 1",
        params![record_id, i64::from(reason)],
    ))?;
    Ok(())
}

/// Total refusals per reason across the vault (`quarantine_status`).
pub fn refused_totals(conn: &Connection) -> Result<Vec<(u8, u64)>, ErrorCode> {
    let mut stmt = db(conn.prepare("SELECT reason, SUM(count) FROM refused_revs GROUP BY reason ORDER BY reason"))?;
    let rows = db(stmt.query_map([], |r| Ok((r.get::<_, i64>(0)? as u8, r.get::<_, i64>(1)? as u64))))?;
    rows.map(db).collect()
}

/// §3.2: next = max(author_hwm, max counter of own held revisions) + 1.
pub fn next_counter(conn: &Connection, record_id: &str, author: &str) -> Result<u64, ErrorCode> {
    let held: Option<i64> = db(conn.query_row(
        "SELECT MAX(counter) FROM record_revs WHERE record_id=?1 AND author_device=?2",
        params![record_id, author],
        |r| r.get(0),
    ))?;
    let hwm: Option<i64> = db(conn
        .query_row("SELECT counter FROM author_hwm WHERE record_id=?1", params![record_id], |r| r.get(0))
        .optional())?;
    Ok(held.unwrap_or(0).max(hwm.unwrap_or(0)) as u64 + 1)
}

/// Record that this device authored `counter` on `record_id` (never lowers).
pub fn bump_hwm(conn: &Connection, record_id: &str, counter: u64) -> Result<(), ErrorCode> {
    db(conn.execute(
        "INSERT INTO author_hwm (record_id, counter) VALUES (?1, ?2)
         ON CONFLICT(record_id) DO UPDATE SET counter = MAX(counter, excluded.counter)",
        params![record_id, counter as i64],
    ))?;
    Ok(())
}

/// Hold a revision until its parents arrive. A second, different copy of
/// a held id is the duplicate case before admission: the record freezes
/// and the first copy is kept (VER-M1).
pub fn put_pending(conn: &Connection, row: &RevisionRow) -> Result<super::merge::MergeOutcome, ErrorCode> {
    let object = crate::backup::object::encode(row)?;
    let held: Option<Vec<u8>> = db(conn
        .query_row("SELECT object FROM pending_revs WHERE revision_id=?1", params![row.revision_id.as_slice()], |r| r.get(0))
        .optional())?;
    match held {
        Some(h) if h == object => Ok(super::merge::MergeOutcome::Pending),
        Some(_) => {
            freeze(conn, &row.record_id, &[row.revision_id])?;
            Ok(super::merge::MergeOutcome::Frozen)
        }
        None => {
            db(conn.execute(
                "INSERT INTO pending_revs (revision_id, record_id, object) VALUES (?1, ?2, ?3)",
                params![row.revision_id.as_slice(), row.record_id, object],
            ))?;
            Ok(super::merge::MergeOutcome::Pending)
        }
    }
}

/// Drop every held revision (VK rotation: they are sealed under the
/// retiring VK and are re-fetched from the next committed state).
pub fn purge_pending(conn: &Connection) -> Result<(), ErrorCode> {
    db(conn.execute("DELETE FROM pending_revs", []))?;
    Ok(())
}

pub fn take_pending(conn: &Connection) -> Result<Vec<Vec<u8>>, ErrorCode> {
    let mut stmt = db(conn.prepare("SELECT object FROM pending_revs ORDER BY revision_id"))?;
    let rows = db(stmt.query_map([], |r| r.get::<_, Vec<u8>>(0)))?;
    let out: Vec<Vec<u8>> = rows.map(db).collect::<Result<_, _>>()?;
    db(conn.execute("DELETE FROM pending_revs", []))?;
    Ok(out)
}

pub fn pending_count(conn: &Connection) -> Result<u64, ErrorCode> {
    let n: i64 = db(conn.query_row("SELECT count(*) FROM pending_revs", [], |r| r.get(0)))?;
    Ok(n as u64)
}
