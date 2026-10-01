//! Revision provenance (spec v0.5 §22.7): every source that delivered a
//! revision — `own` (authored here), `provider` (in a provider-confirmed
//! state's index, or the enrollment bundle), or a peer `device_id`. The
//! re-seal rule and the revocation cutoff read it.

use rusqlite::{params, Connection};

use crate::errors::ErrorCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Own,
    Provider,
    Peer([u8; 16]),
}

impl Source {
    fn bytes(&self) -> Vec<u8> {
        match self {
            Source::Own => b"own".to_vec(),
            Source::Provider => b"provider".to_vec(),
            Source::Peer(id) => id.to_vec(),
        }
    }

    fn from_bytes(b: &[u8]) -> Option<Source> {
        match b {
            b"own" => Some(Source::Own),
            b"provider" => Some(Source::Provider),
            id => id.try_into().ok().map(Source::Peer),
        }
    }
}

pub fn add(conn: &Connection, revision_id: &[u8; 32], source: Source) -> Result<(), ErrorCode> {
    conn.execute("INSERT OR IGNORE INTO rev_sources (revision_id, source) VALUES (?1, ?2)", params![&revision_id[..], source.bytes()])
        .map(|_| ())
        .map_err(|_| ErrorCode::DbCorrupt)
}

pub fn of(conn: &Connection, revision_id: &[u8; 32]) -> Result<Vec<Source>, ErrorCode> {
    let mut stmt = conn.prepare("SELECT source FROM rev_sources WHERE revision_id = ?1").map_err(|_| ErrorCode::DbCorrupt)?;
    let rows = stmt.query_map(params![&revision_id[..]], |r| r.get::<_, Vec<u8>>(0)).map_err(|_| ErrorCode::DbCorrupt)?;
    let mut out = Vec::new();
    for r in rows {
        out.extend(Source::from_bytes(&r.map_err(|_| ErrorCode::DbCorrupt)?));
    }
    Ok(out)
}

/// Re-sealable at a rotation or adoption: an `own` or `provider` source.
pub fn confirmed(conn: &Connection, revision_id: &[u8; 32]) -> Result<bool, ErrorCode> {
    Ok(of(conn, revision_id)?.iter().any(|s| matches!(s, Source::Own | Source::Provider)))
}

/// Delivered only by `peer` (the revocation cutoff, §22.7).
pub fn only_from(conn: &Connection, revision_id: &[u8; 32], peer: &[u8; 16]) -> Result<bool, ErrorCode> {
    let s = of(conn, revision_id)?;
    Ok(!s.is_empty() && s.iter().all(|x| *x == Source::Peer(*peer)))
}
