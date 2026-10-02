//! Whole-table revision helpers used by VK rotation (§2.10) and backup
//! snapshots: read every admitted revision and count live items (§3.4).
//! v0.4: revisions keep their `revision_id` across rotations, so there is
//! no remap and no ordering requirement here.

use rusqlite::Connection;

use super::revisions::{row_from, RevisionRow, ROW_COLUMNS};
use crate::errors::ErrorCode;

pub fn all_rows(conn: &Connection) -> Result<Vec<RevisionRow>, ErrorCode> {
    let mut stmt = conn
        .prepare(&format!("SELECT {ROW_COLUMNS} FROM record_revs ORDER BY revision_id"))
        .map_err(|_| ErrorCode::DbCorrupt)?;
    let rows = stmt.query_map([], row_from).map_err(|_| ErrorCode::DbCorrupt)?;
    rows.map(|r| r.map_err(|_| ErrorCode::DbCorrupt)).collect()
}

/// Live records: non-deleted tips, plus conflicted records that have at
/// least one non-deleted head (§3.4 item-count leak).
pub fn live_count(conn: &Connection) -> Result<u64, ErrorCode> {
    let n: i64 = conn
        .query_row(
            "SELECT count(DISTINCT t.record_id) FROM record_tips t
             JOIN record_revs r ON r.revision_id = t.tip_rev WHERE r.deleted = 0",
            [],
            |r| r.get(0),
        )
        .map_err(|_| ErrorCode::DbCorrupt)?;
    let conflicted: i64 = conn
        .query_row(
            "SELECT count(DISTINCT c.record_id) FROM record_conflicts c
             JOIN record_revs r ON r.revision_id = c.revision_id
             WHERE r.deleted = 0 AND (SELECT tip_rev FROM record_tips
                                      WHERE record_id = c.record_id) IS NULL",
            [],
            |r| r.get(0),
        )
        .map_err(|_| ErrorCode::DbCorrupt)?;
    Ok((n + conflicted) as u64)
}
