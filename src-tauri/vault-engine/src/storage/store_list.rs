//! §1.5 `list_items`: metadata only, never secrets (spec v0.4 §3.2 heads).
//! A record with one non-deleted head lists its metadata; a record with
//! several heads lists as `conflicted`; a frozen record lists as
//! `conflicted` + `tamper`; an undecryptable head lists as `corrupt`
//! (§3.6). Conflicted and frozen records are excluded from fills.

use rusqlite::params;
use serde_json::{json, Value};

use super::records;
use super::rev_state;
use super::revisions::{get_row, heads, RevisionRow};
use super::store::VaultStore;
use crate::crypto::hex;
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;

impl VaultStore {
    pub fn list_records(&self, vk: &SecretBytes<32>) -> Result<Vec<Value>, ErrorCode> {
        let mut stmt = self
            .conn
            .prepare("SELECT record_id FROM record_tips ORDER BY record_id")
            .map_err(|_| ErrorCode::DbCorrupt)?;
        let ids: Vec<String> = stmt
            .query_map(params![], |r| r.get::<_, String>(0))
            .map_err(|_| ErrorCode::DbCorrupt)?
            .collect::<Result<_, _>>()
            .map_err(|_| ErrorCode::DbCorrupt)?;
        let mut items = Vec::new();
        for rid in ids {
            if let Some(item) = self.list_entry(vk, &rid)? {
                items.push(item);
            }
        }
        Ok(items)
    }

    fn list_entry(&self, vk: &SecretBytes<32>, rid: &str) -> Result<Option<Value>, ErrorCode> {
        let hs = heads(&self.conn, rid)?;
        let mut rows = Vec::with_capacity(hs.len());
        for h in &hs {
            rows.push(get_row(&self.conn, h)?.ok_or(ErrorCode::DbCorrupt)?);
        }
        let frozen = rev_state::is_frozen(&self.conn, rid)?;
        if rows.len() == 1 && !frozen {
            let row = &rows[0];
            if row.deleted {
                return Ok(None);
            }
            let kind = records::kind_name(row.kind_tag);
            let Ok(meta) = self.open_row_meta(vk, row) else {
                return Ok(Some(json!({"ref": rid, "kind": kind, "corrupt": true})));
            };
            let meta: Value = serde_json::from_slice(&meta).map_err(|_| ErrorCode::RecordCorrupt)?;
            return Ok(Some(json!({
                "ref": rid,
                "kind": kind,
                "title": meta.get("title").cloned().unwrap_or(Value::Null),
                "username": meta.get("username").cloned().unwrap_or(Value::Null),
                "hosts": meta.get("hosts").cloned().unwrap_or(json!([])),
            })));
        }
        // Conflicted or frozen: hidden only if every head is a tombstone
        // and nothing is frozen.
        if !frozen && rows.iter().all(|r| r.deleted) {
            return Ok(None);
        }
        let kind = rows.iter().find(|r| !r.deleted).or(rows.first()).map(|r| r.kind_tag).unwrap_or(1);
        // Per-head metadata (never secrets) so the review UI can offer
        // `resolve_conflict {chosen_rev}` choices.
        let versions: Vec<Value> = rows.iter().map(|row| self.version_entry(vk, row)).collect();
        let mut item = json!({"ref": rid, "kind": records::kind_name(kind), "conflicted": true, "versions": versions});
        if frozen {
            item["tamper"] = json!(true);
        }
        Ok(Some(item))
    }

    fn version_entry(&self, vk: &SecretBytes<32>, row: &RevisionRow) -> Value {
        let mut v = json!({"rev": hex::encode(row.revision_id), "deleted": row.deleted, "updated_at": row.updated_at});
        match self.open_row_meta(vk, row).ok().and_then(|m| serde_json::from_slice::<Value>(&m).ok()) {
            Some(meta) => {
                v["title"] = meta.get("title").cloned().unwrap_or(Value::Null);
                v["username"] = meta.get("username").cloned().unwrap_or(Value::Null);
            }
            None if !row.deleted => v["corrupt"] = json!(true),
            None => {}
        }
        v
    }
}
