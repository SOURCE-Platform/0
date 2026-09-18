//! Record CRUD on an open `VaultStore` (§3.2 revision model, §2.6
//! encryption). Every mutation is one content-committed revision inside
//! a SQLite transaction, followed by the manifest flip (§2.10).

use rusqlite::params;
use serde_json::{json, Value};

use super::revisions::{self, MergeOutcome, RevisionRow, LOCAL_DEVICE_ID};
use super::store::{now_epoch, VaultStore, META_FIELD_TAG};
use super::{records, revisions::new_record_id};
use crate::crypto::record::{self, RecordCiphertext};
use crate::crypto::secret::{SecretBytes, SecretVec};
use crate::errors::ErrorCode;

pub struct TipPlaintext {
    pub kind_tag: u8,
    pub schema_version: u32,
    pub plaintext: SecretVec,
}

impl VaultStore {
    fn seal_pair(
        &self,
        vk: &SecretBytes<32>,
        record_id: &[u8; 16],
        schema_version: u32,
        plaintext: &[u8],
        meta: &[u8],
    ) -> Result<(RecordCiphertext, RecordCiphertext), ErrorCode> {
        let ct = record::seal_record(
            vk,
            &self.header.vault_id.0,
            record_id,
            schema_version,
            self.header.vk_generation,
            plaintext,
        )
        .map_err(|_| ErrorCode::Internal)?;
        let meta_ct = record::seal_meta(
            vk,
            &self.header.vault_id.0,
            &self.header.meta_salt.0,
            record_id,
            META_FIELD_TAG,
            meta,
        )
        .map_err(|_| ErrorCode::Internal)?;
        Ok((ct, meta_ct))
    }

    fn commit_revision(&mut self, rev: RevisionRow) -> Result<(), ErrorCode> {
        let tx = self.conn.transaction().map_err(|_| ErrorCode::DbCorrupt)?;
        let outcome = revisions::apply_revision(&tx, &rev);
        match outcome {
            Ok(MergeOutcome::FastForward) => {
                tx.commit().map_err(|_| ErrorCode::DbCorrupt)?;
                self.persist_head()
            }
            // Local single-writer ops must always fast-forward; anything
            // else means the merge rules fired unexpectedly.
            Ok(_) => {
                let _ = tx.rollback();
                Err(ErrorCode::Internal)
            }
            Err(e) => {
                let _ = tx.rollback();
                Err(e)
            }
        }
    }

    fn build_rev(
        &self,
        record_id: &str,
        parents: &[[u8; 32]],
        deleted: bool,
        kind_tag: u8,
        schema_version: u32,
        ct: RecordCiphertext,
        meta_ct: RecordCiphertext,
        created_at: u64,
        updated_at: u64,
    ) -> Result<RevisionRow, ErrorCode> {
        let rid = revisions::uuid_bytes(record_id).ok_or(ErrorCode::InvalidInput)?;
        let dev = revisions::uuid_bytes(LOCAL_DEVICE_ID).ok_or(ErrorCode::Internal)?;
        let counter = revisions::next_counter(&self.conn, record_id, LOCAL_DEVICE_ID)?;
        let rev_hash = revisions::rev_hash(
            &rid,
            parents,
            &dev,
            counter,
            deleted,
            &ct.ct,
            &meta_ct.ct,
        );
        Ok(RevisionRow {
            rev_hash,
            record_id: record_id.to_string(),
            parent_revs: parents.to_vec(),
            author_device: LOCAL_DEVICE_ID.to_string(),
            counter,
            deleted,
            kind_tag,
            vk_generation: self.header.vk_generation,
            schema_version,
            nonce: ct.nonce,
            ct: ct.ct,
            meta_nonce: meta_ct.nonce,
            meta_ct: meta_ct.ct,
            created_at,
            updated_at,
        })
    }

    /// Insert a new validated record. Returns the record ref (uuid).
    pub fn add_record(
        &mut self,
        vk: &SecretBytes<32>,
        kind_tag: u8,
        plaintext: &[u8],
        meta: &[u8],
    ) -> Result<String, ErrorCode> {
        let record_id = new_record_id();
        let rid = revisions::uuid_bytes(&record_id).ok_or(ErrorCode::Internal)?;
        let now = now_epoch();
        let (ct, meta_ct) = self.seal_pair(vk, &rid, 1, plaintext, meta)?;
        let rev = self.build_rev(&record_id, &[], false, kind_tag, 1, ct, meta_ct, now, now)?;
        self.commit_revision(rev)?;
        Ok(record_id)
    }

    /// Read and decrypt the current tip of a record (for update merges
    /// and `reveal`). AEAD failure → RECORD_CORRUPT (§3.6 quarantine:
    /// the record is never partially returned).
    pub fn read_tip(
        &self,
        vk: &SecretBytes<32>,
        record_id: &str,
    ) -> Result<TipPlaintext, ErrorCode> {
        let tip = revisions::current_tip(&self.conn, record_id)?.ok_or(ErrorCode::NotFound)?;
        let (kind_tag, schema_version, deleted, nonce, ct): (i64, i64, i64, Vec<u8>, Vec<u8>) = self
            .conn
            .query_row(
                "SELECT kind_tag, schema_version, deleted, nonce, ct FROM record_revs
                 WHERE rev_hash=?1",
                params![tip.as_slice()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .map_err(|_| ErrorCode::DbCorrupt)?;
        if deleted != 0 {
            return Err(ErrorCode::NotFound);
        }
        let rid = revisions::uuid_bytes(record_id).ok_or(ErrorCode::InvalidInput)?;
        let nonce: [u8; 24] = nonce.try_into().map_err(|_| ErrorCode::DbCorrupt)?;
        let plaintext = record::open_record(
            vk,
            &self.header.vault_id.0,
            &rid,
            schema_version as u32,
            self.header.vk_generation,
            &RecordCiphertext { nonce, ct },
        )
        .map_err(|_| ErrorCode::RecordCorrupt)?;
        Ok(TipPlaintext {
            kind_tag: kind_tag as u8,
            schema_version: schema_version as u32,
            plaintext,
        })
    }

    pub fn tip_parent(&self, record_id: &str) -> Result<[u8; 32], ErrorCode> {
        revisions::current_tip(&self.conn, record_id)?.ok_or(ErrorCode::NotFound)
    }

    /// Write the successor revision of the current tip.
    pub fn write_successor(
        &mut self,
        vk: &SecretBytes<32>,
        record_id: &str,
        kind_tag: u8,
        schema_version: u32,
        plaintext: &[u8],
        meta: &[u8],
        created_at: u64,
    ) -> Result<(), ErrorCode> {
        let parent = self.tip_parent(record_id)?;
        let rid = revisions::uuid_bytes(record_id).ok_or(ErrorCode::InvalidInput)?;
        let (ct, meta_ct) = self.seal_pair(vk, &rid, schema_version, plaintext, meta)?;
        let rev = self.build_rev(
            record_id,
            &[parent],
            false,
            kind_tag,
            schema_version,
            ct,
            meta_ct,
            created_at,
            now_epoch(),
        )?;
        self.commit_revision(rev)
    }

    /// Tombstone revision (§3.2). Ciphertexts seal an empty JSON object:
    /// no plaintext remnant of the deleted record survives in the new tip.
    pub fn tombstone(&mut self, vk: &SecretBytes<32>, record_id: &str) -> Result<(), ErrorCode> {
        let tip = self.read_tip(vk, record_id)?; // 404s when absent/deleted
        let parent = self.tip_parent(record_id)?;
        let rid = revisions::uuid_bytes(record_id).ok_or(ErrorCode::InvalidInput)?;
        let (ct, meta_ct) = self.seal_pair(vk, &rid, tip.schema_version, b"{}", b"{}")?;
        let rev = self.build_rev(
            record_id,
            &[parent],
            true,
            tip.kind_tag,
            tip.schema_version,
            ct,
            meta_ct,
            now_epoch(),
            now_epoch(),
        )?;
        self.commit_revision(rev)
    }

    /// §1.5 `list_items`: metadata only, never secrets. Corrupt records
    /// surface as `{ref, corrupt:true}` (§3.6); conflicted records as
    /// `{ref, conflicted:true}` (§15 CONFLICT_PENDING badge class).
    pub fn list_records(&self, vk: &SecretBytes<32>) -> Result<Vec<Value>, ErrorCode> {
        let mut items = Vec::new();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.record_id, r.kind_tag, r.meta_nonce, r.meta_ct
                 FROM record_tips t JOIN record_revs r ON r.rev_hash = t.tip_rev
                 WHERE r.deleted = 0 ORDER BY t.record_id",
            )
            .map_err(|_| ErrorCode::DbCorrupt)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                ))
            })
            .map_err(|_| ErrorCode::DbCorrupt)?;
        for row in rows {
            let (rid_text, kind_tag, meta_nonce, meta_ct) = row.map_err(|_| ErrorCode::DbCorrupt)?;
            items.push(self.meta_entry(vk, &rid_text, kind_tag as u8, meta_nonce, meta_ct)?);
        }
        // Conflicted records (tip NULL): kind known from any branch.
        let mut cstmt = self
            .conn
            .prepare(
                "SELECT t.record_id, (SELECT kind_tag FROM record_revs r
                  JOIN record_conflicts c ON c.rev_hash = r.rev_hash
                  WHERE c.record_id = t.record_id LIMIT 1)
                 FROM record_tips t WHERE t.tip_rev IS NULL",
            )
            .map_err(|_| ErrorCode::DbCorrupt)?;
        let crows = cstmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(|_| ErrorCode::DbCorrupt)?;
        for row in crows {
            let (rid_text, kind_tag) = row.map_err(|_| ErrorCode::DbCorrupt)?;
            items.push(json!({
                "ref": rid_text,
                "kind": records::kind_name(kind_tag as u8),
                "conflicted": true,
            }));
        }
        Ok(items)
    }

    fn meta_entry(
        &self,
        vk: &SecretBytes<32>,
        rid_text: &str,
        kind_tag: u8,
        meta_nonce: Vec<u8>,
        meta_ct: Vec<u8>,
    ) -> Result<Value, ErrorCode> {
        let rid = revisions::uuid_bytes(rid_text).ok_or(ErrorCode::DbCorrupt)?;
        let nonce: [u8; 24] = meta_nonce.try_into().map_err(|_| ErrorCode::DbCorrupt)?;
        let meta = record::open_meta(
            vk,
            &self.header.vault_id.0,
            &self.header.meta_salt.0,
            &rid,
            META_FIELD_TAG,
            &RecordCiphertext { nonce, ct: meta_ct },
        );
        let meta = match meta {
            Ok(m) => m,
            Err(_) => {
                return Ok(json!({
                    "ref": rid_text,
                    "kind": records::kind_name(kind_tag),
                    "corrupt": true,
                }))
            }
        };
        let meta: Value = serde_json::from_slice(&meta).map_err(|_| ErrorCode::RecordCorrupt)?;
        Ok(json!({
            "ref": rid_text,
            "kind": records::kind_name(kind_tag),
            "title": meta.get("title").cloned().unwrap_or(Value::Null),
            "username": meta.get("username").cloned().unwrap_or(Value::Null),
            "hosts": meta.get("hosts").cloned().unwrap_or(json!([])),
        }))
    }
}
