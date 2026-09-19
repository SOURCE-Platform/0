//! Whole-table revision helpers used by VK rotation (§2.10) and backup
//! snapshots: read every revision, order them parents-first, swap a
//! re-sealed row in, and count live items (§3.4).

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection};

use super::revisions::{insert_rev, RevisionRow};
use crate::errors::ErrorCode;

pub fn all_rows(conn: &Connection) -> Result<Vec<RevisionRow>, ErrorCode> {
    let mut stmt = conn
        .prepare(
            "SELECT rev_hash, record_id, parent_revs, author_device, counter, deleted,
                    kind_tag, vk_generation, schema_version, nonce, ct, meta_nonce,
                    meta_ct, created_at, updated_at FROM record_revs",
        )
        .map_err(|_| ErrorCode::DbCorrupt)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, Vec<u8>>(9)?,
                r.get::<_, Vec<u8>>(10)?,
                r.get::<_, Vec<u8>>(11)?,
                r.get::<_, Vec<u8>>(12)?,
                r.get::<_, i64>(13)?,
                r.get::<_, i64>(14)?,
            ))
        })
        .map_err(|_| ErrorCode::DbCorrupt)?;
    let mut out = Vec::new();
    for row in rows {
        let (h, rid, parents, dev, counter, deleted, kind, gen, schema, n, ct, mn, mct, c, u) =
            row.map_err(|_| ErrorCode::DbCorrupt)?;
        if parents.len() % 32 != 0 {
            return Err(ErrorCode::DbCorrupt);
        }
        out.push(RevisionRow {
            rev_hash: to32(&h)?,
            record_id: rid,
            parent_revs: parents.chunks(32).map(to32).collect::<Result<_, _>>()?,
            author_device: dev,
            counter: counter as u64,
            deleted: deleted != 0,
            kind_tag: kind as u8,
            vk_generation: gen as u32,
            schema_version: schema as u32,
            nonce: to24(&n)?,
            ct,
            meta_nonce: to24(&mn)?,
            meta_ct: mct,
            created_at: c as u64,
            updated_at: u as u64,
        });
    }
    Ok(out)
}

fn to32(b: &[u8]) -> Result<[u8; 32], ErrorCode> {
    b.try_into().map_err(|_| ErrorCode::DbCorrupt)
}

fn to24(b: &[u8]) -> Result<[u8; 24], ErrorCode> {
    b.try_into().map_err(|_| ErrorCode::DbCorrupt)
}

/// Indices of `rows` ordered so every parent precedes its children.
/// A parent missing from the table, or a cycle, is corruption.
pub fn topo_order(rows: &[RevisionRow]) -> Result<Vec<usize>, ErrorCode> {
    let index: HashMap<[u8; 32], usize> =
        rows.iter().enumerate().map(|(i, r)| (r.rev_hash, i)).collect();
    let mut done: HashSet<[u8; 32]> = HashSet::new();
    let mut order = Vec::with_capacity(rows.len());
    while order.len() < rows.len() {
        let before = order.len();
        for (i, r) in rows.iter().enumerate() {
            if done.contains(&r.rev_hash) {
                continue;
            }
            if r.parent_revs.iter().any(|p| !index.contains_key(p)) {
                return Err(ErrorCode::DbCorrupt);
            }
            if r.parent_revs.iter().all(|p| done.contains(p)) {
                done.insert(r.rev_hash);
                order.push(i);
            }
        }
        if order.len() == before {
            return Err(ErrorCode::DbCorrupt); // cycle
        }
    }
    Ok(order)
}

/// Replace the row stored under `old_hash` with `row` (new hash).
pub fn replace_row(conn: &Connection, old_hash: &[u8; 32], row: &RevisionRow) -> Result<(), ErrorCode> {
    conn.execute("DELETE FROM record_revs WHERE rev_hash=?1", params![old_hash.as_slice()])
        .map_err(|_| ErrorCode::DbCorrupt)?;
    insert_rev(conn, row)
}

/// Live records: non-deleted tips, plus conflicted records that have at
/// least one non-deleted conflict branch (§3.4 item-count leak).
pub fn live_count(conn: &Connection) -> Result<u64, ErrorCode> {
    let n: i64 = conn
        .query_row(
            "SELECT count(DISTINCT t.record_id) FROM record_tips t
             JOIN record_revs r ON r.rev_hash = t.tip_rev WHERE r.deleted = 0",
            [],
            |r| r.get(0),
        )
        .map_err(|_| ErrorCode::DbCorrupt)?;
    let conflicted: i64 = conn
        .query_row(
            "SELECT count(DISTINCT c.record_id) FROM record_conflicts c
             JOIN record_revs r ON r.rev_hash = c.rev_hash
             WHERE r.deleted = 0 AND (SELECT tip_rev FROM record_tips
                                      WHERE record_id = c.record_id) IS NULL",
            [],
            |r| r.get(0),
        )
        .map_err(|_| ErrorCode::DbCorrupt)?;
    Ok((n + conflicted) as u64)
}
