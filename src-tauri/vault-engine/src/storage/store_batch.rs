//! Many new records in one transaction and one manifest flip (Phase I,
//! spec §10.4). `commit_local` flips per revision, and a flip rewrites the
//! manifest's full object list — fine for one item, quadratic for an
//! import. The import seals each row at once (so no plaintext waits) and
//! commits the sealed revisions in batches; each batch is all-or-nothing,
//! together with its `import_log` rows.

use super::merge::{self, MergeOutcome, NoCompare};
use super::rev_state;
use super::revisions::{new_record_id, RevisionRow};
use super::store::{now_epoch, VaultStore};
use super::store_records::NewRevision;
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;

impl VaultStore {
    /// Seal a brand-new record's first revision without applying it.
    pub fn seal_new(&self, vk: &SecretBytes<32>, kind_tag: u8, plaintext: &[u8], meta: &[u8]) -> Result<RevisionRow, ErrorCode> {
        let record_id = new_record_id();
        let n = NewRevision {
            record_id: &record_id,
            parents: Vec::new(),
            deleted: false,
            kind_tag,
            schema_version: 1,
            plaintext,
            meta,
            created_at: now_epoch(),
        };
        self.author(vk, n)
    }

    /// Apply sealed new-record revisions, run `extra` in the same
    /// transaction, then flip once. Any failure rolls the batch back.
    pub fn commit_new_batch(
        &mut self,
        revs: &[RevisionRow],
        extra: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<(), ErrorCode>,
    ) -> Result<(), ErrorCode> {
        if revs.is_empty() {
            return Ok(());
        }
        let target = self.flip_target(self.header.clone());
        let generation = self.header.vk_generation;
        let tx = self.conn.transaction().map_err(|_| ErrorCode::DbCorrupt)?;
        for rev in revs {
            match merge::apply_revision(&tx, rev, generation, &NoCompare)? {
                MergeOutcome::Applied { conflicted: false } => {}
                _ => return Err(ErrorCode::Internal), // dropping `tx` rolls back
            }
            rev_state::bump_hwm(&tx, &rev.record_id, rev.counter)?;
            super::sources::add(&tx, &rev.revision_id, super::sources::Source::Own)?;
        }
        extra(&tx)?;
        super::flip::stamp(&tx, &target)?;
        tx.commit().map_err(|_| ErrorCode::DbCorrupt)?;
        self.persist_head()
    }
}
