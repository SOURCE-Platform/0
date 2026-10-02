//! Revision identity and row storage (spec v0.4 §3.2).
//!
//! A revision's logical identity is a random 256-bit `revision_id`,
//! created when it is authored and never changed; parents are
//! `revision_id`s. Storage and backup addressing use a separate
//! `blob_hash` (§3.7), so a VK rotation re-seals a revision without
//! renaming the graph. The per-revision `graph_digest` is bound into the
//! record/meta AAD (§2.6). The merge rules live in `storage::merge`.

use rusqlite::{params, Connection, OptionalExtension};

use crate::errors::ErrorCode;

// The revision identity (RevisionRow, graph_digest, id helpers) lives in
// `vault-proto` (§1.1); this module adds the SQLite storage.
pub use vault_proto::rev::*;

fn join_ids(ids: &[[u8; 32]]) -> Vec<u8> {
    ids.iter().flat_map(|p| p.iter().copied()).collect()
}

pub fn split_ids(blob: &[u8]) -> Result<Vec<[u8; 32]>, ErrorCode> {
    if !blob.len().is_multiple_of(32) {
        return Err(ErrorCode::DbCorrupt);
    }
    blob.chunks(32).map(|c| c.try_into().map_err(|_| ErrorCode::DbCorrupt)).collect()
}

pub fn insert_rev(conn: &Connection, rev: &RevisionRow) -> Result<(), ErrorCode> {
    conn.execute(
        "INSERT OR REPLACE INTO record_revs
         (revision_id, record_id, parent_ids, author_device, counter, deleted,
          kind_tag, vk_generation, schema_version, nonce, ct, meta_nonce,
          meta_ct, created_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        params![
            rev.revision_id.as_slice(),
            rev.record_id,
            join_ids(&rev.parent_ids),
            rev.author_device,
            rev.counter as i64,
            i64::from(rev.deleted),
            i64::from(rev.kind_tag),
            i64::from(rev.vk_generation),
            i64::from(rev.schema_version),
            rev.nonce.as_slice(),
            rev.ct,
            rev.meta_nonce.as_slice(),
            rev.meta_ct,
            rev.created_at as i64,
            rev.updated_at as i64,
        ],
    )
    .map_err(|_| ErrorCode::DbCorrupt)?;
    Ok(())
}

pub const ROW_COLUMNS: &str = "revision_id, record_id, parent_ids, author_device, counter, deleted,
    kind_tag, vk_generation, schema_version, nonce, ct, meta_nonce, meta_ct, created_at, updated_at";

pub fn row_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<RevisionRow> {
    let id: Vec<u8> = r.get(0)?;
    let parents: Vec<u8> = r.get(2)?;
    let nonce: Vec<u8> = r.get(9)?;
    let meta_nonce: Vec<u8> = r.get(11)?;
    let bad = |_| rusqlite::Error::InvalidQuery;
    Ok(RevisionRow {
        revision_id: id.as_slice().try_into().map_err(bad)?,
        record_id: r.get(1)?,
        parent_ids: split_ids(&parents).map_err(|_| rusqlite::Error::InvalidQuery)?,
        author_device: r.get(3)?,
        counter: r.get::<_, i64>(4)? as u64,
        deleted: r.get::<_, i64>(5)? != 0,
        kind_tag: r.get::<_, i64>(6)? as u8,
        vk_generation: r.get::<_, i64>(7)? as u32,
        schema_version: r.get::<_, i64>(8)? as u32,
        nonce: nonce.as_slice().try_into().map_err(bad)?,
        ct: r.get(10)?,
        meta_nonce: meta_nonce.as_slice().try_into().map_err(bad)?,
        meta_ct: r.get(12)?,
        created_at: r.get::<_, i64>(13)? as u64,
        updated_at: r.get::<_, i64>(14)? as u64,
    })
}

pub fn get_row(conn: &Connection, id: &[u8; 32]) -> Result<Option<RevisionRow>, ErrorCode> {
    conn.query_row(
        &format!("SELECT {ROW_COLUMNS} FROM record_revs WHERE revision_id=?1"),
        params![id.as_slice()],
        row_from,
    )
    .optional()
    .map_err(|_| ErrorCode::DbCorrupt)
}

/// Current heads of a record: the single tip, or the conflict set.
pub fn heads(conn: &Connection, record_id: &str) -> Result<Vec<[u8; 32]>, ErrorCode> {
    let tip: Option<Option<Vec<u8>>> = conn
        .query_row("SELECT tip_rev FROM record_tips WHERE record_id=?1", params![record_id], |r| r.get(0))
        .optional()
        .map_err(|_| ErrorCode::DbCorrupt)?;
    match tip {
        None => Ok(Vec::new()),
        Some(Some(t)) => Ok(vec![t.as_slice().try_into().map_err(|_| ErrorCode::DbCorrupt)?]),
        Some(None) => {
            let mut stmt = conn
                .prepare("SELECT revision_id FROM record_conflicts WHERE record_id=?1 ORDER BY revision_id")
                .map_err(|_| ErrorCode::DbCorrupt)?;
            let ids = stmt
                .query_map(params![record_id], |r| r.get::<_, Vec<u8>>(0))
                .map_err(|_| ErrorCode::DbCorrupt)?;
            ids.map(|b| b.map_err(|_| ErrorCode::DbCorrupt)?.as_slice().try_into().map_err(|_| ErrorCode::DbCorrupt))
                .collect()
        }
    }
}

/// Replace a record's heads (one → tip; several → conflict set).
pub fn set_heads(conn: &Connection, record_id: &str, heads: &[[u8; 32]]) -> Result<(), ErrorCode> {
    let q = |sql: &str, p: &[&dyn rusqlite::ToSql]| conn.execute(sql, p).map(|_| ()).map_err(|_| ErrorCode::DbCorrupt);
    q("DELETE FROM record_conflicts WHERE record_id=?1", &[&record_id])?;
    let tip: Option<Vec<u8>> = (heads.len() == 1).then(|| heads[0].to_vec());
    q(
        "INSERT INTO record_tips (record_id, tip_rev) VALUES (?1, ?2)
         ON CONFLICT(record_id) DO UPDATE SET tip_rev=excluded.tip_rev",
        &[&record_id, &tip],
    )?;
    if heads.len() > 1 {
        for h in heads {
            q("INSERT INTO record_conflicts (record_id, revision_id) VALUES (?1, ?2)", &[&record_id, &h.to_vec()])?;
        }
    }
    Ok(())
}
