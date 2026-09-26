//! Record authoring on an open `VaultStore` (spec v0.4 §3.2 revision
//! model, §2.6 encryption). Every mutation is one revision with a fresh
//! 256-bit `revision_id`, authored under this device's registry id, whose
//! parents are the record's current heads; it is applied through the §3.2
//! merge rules inside one SQLite transaction, then the manifest flips.

use super::merge::{self, MergeOutcome, NoCompare};
use super::rev_state;
use super::revisions::{self, get_row, heads, new_record_id, new_revision_id, RevisionRow};
use super::store::{now_epoch, VaultStore, META_FIELD_TAG};
use crate::crypto::record::{self, RecordCiphertext};
use crate::crypto::secret::{SecretBytes, SecretVec};
use crate::errors::ErrorCode;
use zeroize::Zeroizing;

pub struct TipPlaintext {
    pub kind_tag: u8,
    pub schema_version: u32,
    pub plaintext: SecretVec,
}

/// What a `resolve_conflict` revision carries (§1.5, §3.2).
pub enum Resolution<'a> {
    /// Keep the content of one of the current heads.
    Chosen([u8; 32]),
    /// New content supplied by the user (record JSON + metadata JSON),
    /// edited from the current head `base`.
    Edited { base: [u8; 32], kind_tag: u8, schema_version: u32, plaintext: &'a [u8], meta: &'a [u8] },
}

pub struct NewRevision<'a> {
    pub record_id: &'a str,
    pub parents: Vec<[u8; 32]>,
    pub deleted: bool,
    pub kind_tag: u8,
    pub schema_version: u32,
    pub plaintext: &'a [u8],
    pub meta: &'a [u8],
    pub created_at: u64,
}

impl VaultStore {
    /// Seal and assemble a revision authored by this device.
    fn author(&self, vk: &SecretBytes<32>, n: NewRevision<'_>) -> Result<RevisionRow, ErrorCode> {
        let rid = revisions::uuid_bytes(n.record_id).ok_or(ErrorCode::InvalidInput)?;
        let author = self.author_device()?;
        let mut parents = n.parents;
        parents.sort();
        parents.dedup();
        let mut row = RevisionRow {
            revision_id: new_revision_id(),
            record_id: n.record_id.to_string(),
            parent_ids: parents,
            counter: rev_state::next_counter(&self.conn, n.record_id, &author)?,
            author_device: author,
            deleted: n.deleted,
            kind_tag: n.kind_tag,
            vk_generation: self.header.vk_generation,
            schema_version: n.schema_version,
            nonce: [0; 24],
            ct: Vec::new(),
            meta_nonce: [0; 24],
            meta_ct: Vec::new(),
            created_at: n.created_at,
            updated_at: now_epoch(),
        };
        let bind = row.bind()?;
        let vid = self.header.vault_id.0;
        let ct = record::seal_record(vk, &vid, &rid, &bind, n.schema_version, row.vk_generation, n.plaintext)
            .map_err(|_| ErrorCode::Internal)?;
        let meta_ct = record::seal_meta(vk, &vid, &self.header.meta_salt.0, &rid, &bind, META_FIELD_TAG, n.meta)
            .map_err(|_| ErrorCode::Internal)?;
        (row.nonce, row.ct, row.meta_nonce, row.meta_ct) = (ct.nonce, ct.ct, meta_ct.nonce, meta_ct.ct);
        Ok(row)
    }

    /// Apply a locally authored revision. It must leave exactly one head,
    /// except a resolution over more heads than one revision may name
    /// (`allow_conflict`, §3.2 partial cover).
    fn commit_local(&mut self, rev: RevisionRow, unfreeze: bool, allow_conflict: bool) -> Result<(), ErrorCode> {
        let target = self.flip_target(self.header.clone());
        let tx = self.conn.transaction().map_err(|_| ErrorCode::DbCorrupt)?;
        match merge::apply_revision(&tx, &rev, self.header.vk_generation, &NoCompare) {
            Ok(MergeOutcome::Applied { conflicted: false }) => {}
            Ok(MergeOutcome::Applied { conflicted: true }) if allow_conflict => {}
            Ok(_) => {
                let _ = tx.rollback();
                return Err(ErrorCode::Internal);
            }
            Err(e) => {
                let _ = tx.rollback();
                return Err(e);
            }
        }
        rev_state::bump_hwm(&tx, &rev.record_id, rev.counter)?;
        if unfreeze {
            rev_state::unfreeze(&tx, &rev.record_id)?;
        }
        super::flip::stamp(&tx, &target)?;
        tx.commit().map_err(|_| ErrorCode::DbCorrupt)?;
        self.persist_head()
    }

    /// The single current head of an editable record. Conflicted or frozen
    /// records → CONFLICT_PENDING; absent or tombstoned → NOT_FOUND.
    fn editable_head(&self, record_id: &str) -> Result<RevisionRow, ErrorCode> {
        if rev_state::is_frozen(&self.conn, record_id)? {
            return Err(ErrorCode::ConflictPending);
        }
        let hs = heads(&self.conn, record_id)?;
        match hs.as_slice() {
            [] => Err(ErrorCode::NotFound),
            [one] => {
                let row = get_row(&self.conn, one)?.ok_or(ErrorCode::DbCorrupt)?;
                if row.deleted {
                    Err(ErrorCode::NotFound)
                } else {
                    Ok(row)
                }
            }
            _ => Err(ErrorCode::ConflictPending),
        }
    }

    /// Decrypt one revision's record plaintext.
    pub fn open_row(&self, vk: &SecretBytes<32>, row: &RevisionRow) -> Result<SecretVec, ErrorCode> {
        let rid = revisions::uuid_bytes(&row.record_id).ok_or(ErrorCode::DbCorrupt)?;
        record::open_record(
            vk,
            &self.header.vault_id.0,
            &rid,
            &row.bind()?,
            row.schema_version,
            row.vk_generation,
            &RecordCiphertext { nonce: row.nonce, ct: row.ct.clone() },
        )
        .map_err(|_| ErrorCode::RecordCorrupt)
    }

    /// Decrypt one revision's metadata plaintext.
    pub fn open_row_meta(&self, vk: &SecretBytes<32>, row: &RevisionRow) -> Result<SecretVec, ErrorCode> {
        let rid = revisions::uuid_bytes(&row.record_id).ok_or(ErrorCode::DbCorrupt)?;
        record::open_meta(
            vk,
            &self.header.vault_id.0,
            &self.header.meta_salt.0,
            &rid,
            &row.bind()?,
            META_FIELD_TAG,
            &RecordCiphertext { nonce: row.meta_nonce, ct: row.meta_ct.clone() },
        )
        .map_err(|_| ErrorCode::RecordCorrupt)
    }

    /// Insert a new validated record. Returns the record ref (uuid).
    pub fn add_record(&mut self, vk: &SecretBytes<32>, kind_tag: u8, plaintext: &[u8], meta: &[u8]) -> Result<String, ErrorCode> {
        let record_id = new_record_id();
        let rev = self.author(
            vk,
            NewRevision {
                record_id: &record_id,
                parents: Vec::new(),
                deleted: false,
                kind_tag,
                schema_version: 1,
                plaintext,
                meta,
                created_at: now_epoch(),
            },
        )?;
        self.commit_local(rev, false, false)?;
        Ok(record_id)
    }

    /// Read and decrypt the single current head (for update merges and
    /// `reveal`). AEAD failure → RECORD_CORRUPT (§3.6 quarantine).
    pub fn read_tip(&self, vk: &SecretBytes<32>, record_id: &str) -> Result<TipPlaintext, ErrorCode> {
        let row = self.editable_head(record_id)?;
        Ok(TipPlaintext { kind_tag: row.kind_tag, schema_version: row.schema_version, plaintext: self.open_row(vk, &row)? })
    }

    /// Write the successor revision of the single current head.
    #[allow(clippy::too_many_arguments)]
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
        let head = self.editable_head(record_id)?;
        let rev = self.author(
            vk,
            NewRevision {
                record_id,
                parents: vec![head.revision_id],
                deleted: false,
                kind_tag,
                schema_version,
                plaintext,
                meta,
                created_at,
            },
        )?;
        self.commit_local(rev, false, false)
    }

    /// Tombstone revision (§3.2). Ciphertexts seal an empty JSON object:
    /// no plaintext remnant of the deleted record survives in the new head.
    pub fn tombstone(&mut self, vk: &SecretBytes<32>, record_id: &str) -> Result<(), ErrorCode> {
        let head = self.editable_head(record_id)?;
        let rev = self.author(
            vk,
            NewRevision {
                record_id,
                parents: vec![head.revision_id],
                deleted: true,
                kind_tag: head.kind_tag,
                schema_version: head.schema_version,
                plaintext: b"{}",
                meta: b"{}",
                created_at: head.created_at,
            },
        )?;
        self.commit_local(rev, false, false)
    }

    /// §3.2 descendants: this device's own revision whose ancestry was
    /// refused is re-authored — same content, a new `revision_id`, parents
    /// = the record's current heads (it may join a conflict).
    pub fn reauthor(&mut self, vk: &SecretBytes<32>, old: &RevisionRow) -> Result<(), ErrorCode> {
        let pt = self.open_row(vk, old)?;
        let meta = Zeroizing::new(self.open_row_meta(vk, old)?.to_vec());
        let parents = heads(&self.conn, &old.record_id)?;
        let rev = self.author(
            vk,
            NewRevision {
                record_id: &old.record_id,
                parents,
                deleted: false,
                kind_tag: old.kind_tag,
                schema_version: old.schema_version,
                plaintext: &pt,
                meta: &meta,
                created_at: old.created_at,
            },
        )?;
        self.commit_local(rev, false, true)
    }

    /// `resolve_conflict` (§3.2): a revision whose parents are the chosen
    /// head plus the other current heads — at most `MAX_PARENTS` in all,
    /// lowest ids first; any beyond stay in conflict (partial cover). A
    /// frozen record also needs the user's acknowledgement, checked here as
    /// well as before the presence prompt (VER-I5).
    pub fn resolve(
        &mut self,
        vk: &SecretBytes<32>,
        record_id: &str,
        res: Resolution<'_>,
        acknowledge_tamper: bool,
    ) -> Result<(), ErrorCode> {
        let hs = heads(&self.conn, record_id)?;
        let frozen = rev_state::is_frozen(&self.conn, record_id)?;
        if hs.is_empty() || (hs.len() < 2 && !frozen) {
            return Err(ErrorCode::BadState);
        }
        if frozen && !acknowledge_tamper {
            return Err(ErrorCode::ConflictPending);
        }
        let base = match &res {
            Resolution::Chosen(id) | Resolution::Edited { base: id, .. } => *id,
        };
        if !hs.contains(&base) {
            return Err(ErrorCode::InvalidInput);
        }
        let row = get_row(&self.conn, &base)?.ok_or(ErrorCode::DbCorrupt)?;
        // A frozen record whose one head is a tombstone chosen as-is: any
        // new revision would leave the tombstone beside it, so the
        // acknowledged resolution only clears the freeze (SEC-I10).
        if hs.len() == 1 && row.deleted && matches!(res, Resolution::Chosen(_)) {
            let tx = self.conn.transaction().map_err(|_| ErrorCode::DbCorrupt)?;
            rev_state::unfreeze(&tx, record_id)?;
            return tx.commit().map_err(|_| ErrorCode::DbCorrupt);
        }
        let (kind_tag, schema_version, plaintext, meta, created_at, deleted) = match res {
            Resolution::Chosen(_) => {
                let pt = self.open_row(vk, &row)?;
                let meta = self.open_row_meta(vk, &row)?;
                (row.kind_tag, row.schema_version, Zeroizing::new(pt.to_vec()), Zeroizing::new(meta.to_vec()), row.created_at, row.deleted)
            }
            Resolution::Edited { kind_tag, schema_version, plaintext, meta, .. } => {
                if row.deleted {
                    return Err(ErrorCode::InvalidInput);
                }
                (kind_tag, schema_version, Zeroizing::new(plaintext.to_vec()), Zeroizing::new(meta.to_vec()), now_epoch(), false)
            }
        };
        // Parents: the chosen head, then every head this device authored
        // (leaving one out would read as an author fork, SEC-I9), then the
        // lowest others — at most MAX_PARENTS; any beyond stay in conflict.
        let me = self.author_device()?;
        let mut others: Vec<([u8; 32], bool)> = Vec::new();
        for h in hs.iter().filter(|h| **h != base) {
            let mine = get_row(&self.conn, h)?.is_some_and(|r| r.author_device == me);
            others.push((*h, mine));
        }
        others.sort_by_key(|(h, mine)| (!*mine, *h));
        let mut parents = vec![base];
        parents.extend(others.iter().map(|(h, _)| *h).take(revisions::MAX_PARENTS - 1));
        let partial = parents.len() < hs.len();
        let rev = self.author(
            vk,
            NewRevision { record_id, parents, deleted, kind_tag, schema_version, plaintext: &plaintext, meta: &meta, created_at },
        )?;
        self.commit_local(rev, frozen, partial)
    }
}
