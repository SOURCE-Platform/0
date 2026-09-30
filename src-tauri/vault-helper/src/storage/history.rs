//! Retained history (spec §22.4): list a record's revisions, list
//! tombstoned records, and restore a retained revision's content. A
//! restore never resurrects a tombstoned record (§3.2): it creates a new
//! record carrying the old content, so every device merges it as a plain
//! addition. Metadata only leaves this module; never secrets.

use rusqlite::params;
use serde_json::{json, Value};

use super::records;
use super::revisions::{get_row, heads, row_from, RevisionRow, ROW_COLUMNS};
use super::store::VaultStore;
use crate::crypto::hex;
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;

impl VaultStore {
    fn record_rows(&self, record_id: &str) -> Result<Vec<RevisionRow>, ErrorCode> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {ROW_COLUMNS} FROM record_revs WHERE record_id = ?1 ORDER BY updated_at, revision_id"))
            .map_err(|_| ErrorCode::DbCorrupt)?;
        let rows = stmt.query_map(params![record_id], row_from).map_err(|_| ErrorCode::DbCorrupt)?;
        rows.map(|r| r.map_err(|_| ErrorCode::DbCorrupt)).collect()
    }

    /// Every retained revision of one record, oldest first.
    pub fn list_history(&self, record_id: &str) -> Result<Vec<Value>, ErrorCode> {
        let rows = self.record_rows(record_id)?;
        if rows.is_empty() {
            return Err(ErrorCode::NotFound);
        }
        let hs = heads(&self.conn, record_id)?;
        Ok(rows
            .iter()
            .map(|r| {
                json!({
                    "revision_id": hex::encode(r.revision_id),
                    "author_device": r.author_device,
                    "updated_at": r.updated_at,
                    "deleted": r.deleted,
                    "current": hs.contains(&r.revision_id),
                })
            })
            .collect())
    }

    /// Records whose single head is a tombstone, with the title of the
    /// last revision that still had one.
    pub fn list_deleted(&self, vk: &SecretBytes<32>) -> Result<Vec<Value>, ErrorCode> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.record_id FROM record_tips t JOIN record_revs r ON r.revision_id = t.tip_rev
                 WHERE r.deleted = 1 ORDER BY t.record_id",
            )
            .map_err(|_| ErrorCode::DbCorrupt)?;
        let ids: Vec<String> = stmt
            .query_map(params![], |r| r.get::<_, String>(0))
            .map_err(|_| ErrorCode::DbCorrupt)?
            .collect::<Result<_, _>>()
            .map_err(|_| ErrorCode::DbCorrupt)?;
        let mut out = Vec::new();
        for rid in ids {
            let rows = self.record_rows(&rid)?;
            let Some(last) = rows.iter().rev().find(|r| !r.deleted) else {
                continue;
            };
            let title = self
                .open_row_meta(vk, last)
                .ok()
                .and_then(|m| serde_json::from_slice::<Value>(&m).ok())
                .and_then(|m| m.get("title").cloned())
                .unwrap_or(Value::Null);
            out.push(json!({
                "ref": rid,
                "kind": records::kind_name(last.kind_tag),
                "title": title,
                "revision_id": hex::encode(last.revision_id),
            }));
        }
        Ok(out)
    }

    /// Restore a retained, non-deleted revision's content. Live record →
    /// an ordinary successor revision; tombstoned record → a new record.
    /// Returns the ref that now carries the content.
    pub fn restore_revision(&mut self, vk: &SecretBytes<32>, record_id: &str, revision_id: &[u8; 32]) -> Result<String, ErrorCode> {
        let row = get_row(&self.conn, revision_id)?.filter(|r| r.record_id == record_id && !r.deleted).ok_or(ErrorCode::NotFound)?;
        let plaintext = self.open_row(vk, &row)?;
        let meta = self.open_row_meta(vk, &row)?;
        let hs = heads(&self.conn, record_id)?;
        let tombstoned = match hs.as_slice() {
            [one] => get_row(&self.conn, one)?.ok_or(ErrorCode::DbCorrupt)?.deleted,
            _ => false,
        };
        if tombstoned {
            return self.add_record(vk, row.kind_tag, &plaintext, &meta);
        }
        // Conflicted or frozen records refuse here (`CONFLICT_PENDING`).
        self.write_successor(vk, record_id, row.kind_tag, row.schema_version, &plaintext, &meta, row.created_at)?;
        Ok(record_id.to_string())
    }
}
