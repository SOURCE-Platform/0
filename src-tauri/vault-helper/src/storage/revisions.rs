//! Content-committed revision graph (spec §3.2). Merge rules are
//! deterministic and timestamp-free: fast-forward, concurrent→conflict,
//! tombstone conservatism, equivocation detection.

use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

use crate::errors::ErrorCode;

/// Phase C has no enrolled device identity (Phase E). Locally authored
/// revisions carry the all-zero device id until enrollment assigns the
/// real one; the id is content-committed into `rev_hash`, so existing
/// revisions stay valid when the real id appears.
pub const LOCAL_DEVICE_ID: &str = "00000000-0000-0000-0000-000000000000";

#[derive(Debug, Clone)]
pub struct RevisionRow {
    pub rev_hash: [u8; 32],
    pub record_id: String,
    pub parent_revs: Vec<[u8; 32]>,
    pub author_device: String,
    pub counter: u64,
    pub deleted: bool,
    pub kind_tag: u8,
    pub vk_generation: u32,
    pub schema_version: u32,
    pub nonce: [u8; 24],
    pub ct: Vec<u8>,
    pub meta_nonce: [u8; 24],
    pub meta_ct: Vec<u8>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// §3.2: SHA-256("ov0/rev/v1" ‖ record_id ‖ parent_revs ‖ author_device
/// ‖ u64be(counter) ‖ u8(deleted) ‖ SHA-256(ct) ‖ SHA-256(meta_ct)).
/// `record_id`/`author_device` are the 16 raw uuid bytes.
pub fn rev_hash(
    record_id: &[u8; 16],
    parent_revs: &[[u8; 32]],
    author_device: &[u8; 16],
    counter: u64,
    deleted: bool,
    ct: &[u8],
    meta_ct: &[u8],
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"ov0/rev/v1");
    h.update(record_id);
    for p in parent_revs {
        h.update(p);
    }
    h.update(author_device);
    h.update(counter.to_be_bytes());
    h.update([u8::from(deleted)]);
    h.update(Sha256::digest(ct));
    h.update(Sha256::digest(meta_ct));
    h.finalize().into()
}

pub fn uuid_bytes(uuid: &str) -> Option<[u8; 16]> {
    let hex: String = uuid.chars().filter(|c| *c != '-').collect();
    crate::crypto::hex::decode_array(&hex)
}

/// Random UUIDv4 text (§3.3: content-independent).
pub fn new_record_id() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("OS CSPRNG failure is unrecoverable");
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // variant 10
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13],
        b[14], b[15]
    )
}

/// Next counter for (record_id, author_device) — §3.2 monotonic rule.
pub fn next_counter(
    conn: &Connection,
    record_id: &str,
    author_device: &str,
) -> Result<u64, ErrorCode> {
    let max: Option<i64> = conn
        .query_row(
            "SELECT MAX(counter) FROM record_revs WHERE record_id=?1 AND author_device=?2",
            params![record_id, author_device],
            |r| r.get(0),
        )
        .map_err(|_| ErrorCode::DbCorrupt)?;
    Ok(max.map(|m| m as u64 + 1).unwrap_or(1))
}

pub fn current_tip(conn: &Connection, record_id: &str) -> Result<Option<[u8; 32]>, ErrorCode> {
    conn.query_row(
        "SELECT tip_rev FROM record_tips WHERE record_id=?1",
        params![record_id],
        |r| r.get::<_, Option<Vec<u8>>>(0),
    )
    .map(|opt| {
        opt.and_then(|v| <[u8; 32]>::try_from(v.as_slice()).ok())
    })
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        _ => Err(ErrorCode::DbCorrupt),
    })
}

pub fn is_deleted(conn: &Connection, rev_hash: &[u8; 32]) -> Result<bool, ErrorCode> {
    conn.query_row(
        "SELECT deleted FROM record_revs WHERE rev_hash=?1",
        params![rev_hash.as_slice()],
        |r| r.get::<_, i64>(0),
    )
    .map(|d| d != 0)
    .map_err(|_| ErrorCode::DbCorrupt)
}

#[derive(Debug, PartialEq, Eq)]
pub enum MergeOutcome {
    /// Revision fast-forwarded and is now the tip.
    FastForward,
    /// Exact duplicate of a known revision — idempotent no-op.
    AlreadyKnown,
    /// Concurrent edit, tombstone race, or equivocation: tip NULLed,
    /// revisions in `record_conflicts`.
    Conflict,
}

/// Apply one revision under the §3.2 merge rules. Caller supplies a
/// transaction. Phase C authors only fast-forwards locally; the full rule
/// set is here (and unit-tested) so Phase F sync adds no new merge code.
pub fn apply_revision(conn: &Connection, rev: &RevisionRow) -> Result<MergeOutcome, ErrorCode> {
    let exists: bool = conn
        .query_row(
            "SELECT count(*) FROM record_revs WHERE rev_hash=?1",
            params![rev.rev_hash.as_slice()],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|_| ErrorCode::DbCorrupt)?
        > 0;
    if exists {
        return Ok(MergeOutcome::AlreadyKnown);
    }
    // Equivocation: same (record, author, counter), different hash.
    let equivocation: bool = conn
        .query_row(
            "SELECT count(*) FROM record_revs
             WHERE record_id=?1 AND author_device=?2 AND counter=?3 AND rev_hash != ?4",
            params![
                rev.record_id,
                rev.author_device,
                rev.counter as i64,
                rev.rev_hash.as_slice()
            ],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|_| ErrorCode::DbCorrupt)?
        > 0;
    insert_rev(conn, rev)?;
    let tip = current_tip(conn, &rev.record_id)?;
    let fast_forward = match tip {
        None => rev.parent_revs.is_empty(),
        Some(t) => rev.parent_revs.contains(&t),
    };
    // Conservative tombstone rule: an edit claiming causal ancestry
    // after a tombstone is a conflict, never a resurrection (§3.2).
    let resurrecting = !rev.deleted
        && rev
            .parent_revs
            .iter()
            .any(|p| is_deleted(conn, p).unwrap_or(false));
    if equivocation || !fast_forward || resurrecting {
        conn.execute(
            "UPDATE record_tips SET tip_rev=NULL WHERE record_id=?1",
            params![rev.record_id],
        )
        .map_err(|_| ErrorCode::DbCorrupt)?;
        conn.execute(
            "INSERT OR IGNORE INTO record_conflicts (record_id, rev_hash) VALUES (?1, ?2)",
            params![rev.record_id, rev.rev_hash.as_slice()],
        )
        .map_err(|_| ErrorCode::DbCorrupt)?;
        if let Some(t) = tip {
            conn.execute(
                "INSERT OR IGNORE INTO record_conflicts (record_id, rev_hash) VALUES (?1, ?2)",
                params![rev.record_id, t.as_slice()],
            )
            .map_err(|_| ErrorCode::DbCorrupt)?;
        }
        return Ok(MergeOutcome::Conflict);
    }
    conn.execute(
        "INSERT INTO record_tips (record_id, tip_rev) VALUES (?1, ?2)
         ON CONFLICT(record_id) DO UPDATE SET tip_rev=excluded.tip_rev",
        params![rev.record_id, rev.rev_hash.as_slice()],
    )
    .map_err(|_| ErrorCode::DbCorrupt)?;
    Ok(MergeOutcome::FastForward)
}

pub(super) fn insert_rev(conn: &Connection, rev: &RevisionRow) -> Result<(), ErrorCode> {
    let mut parents = Vec::with_capacity(rev.parent_revs.len() * 32);
    for p in &rev.parent_revs {
        parents.extend_from_slice(p);
    }
    conn.execute(
        "INSERT INTO record_revs
         (rev_hash, record_id, parent_revs, author_device, counter, deleted,
          kind_tag, vk_generation, schema_version, nonce, ct, meta_nonce,
          meta_ct, created_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        params![
            rev.rev_hash.as_slice(),
            rev.record_id,
            parents,
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
