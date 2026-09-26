//! Crash-safe header + manifest flips (spec §3.5; review SEC-I7, SEC-O8,
//! SEC-O9, SEC-O10). Every flip first records its complete target header
//! in `kv` — inside the caller's transaction when the revision set changes
//! with it — then writes `header.json` and `manifest.json`, and only then
//! updates the in-memory store. A crash anywhere in between leaves a vault
//! that `open` rolls forward to exactly the recorded target, after checking
//! the candidate pair in memory; any other inconsistency fails closed.

use rusqlite::Connection;

use super::header::{self, Header};
use super::kv;
use super::manifest::{self, check_against_header, Manifest, ManifestObject, MANIFEST_NAME};
use super::revision_rows;
use super::store::{write_atomic, VaultStore};
use crate::crypto::hex;
use crate::errors::ErrorCode;
use crate::VAULT_HEADER_NAME;

const KEY: &str = "flip_target";

/// Record the next flip's complete header (joins the caller's transaction).
pub fn stamp(conn: &Connection, next: &Header) -> Result<(), ErrorCode> {
    kv::put(conn, KEY, next)
}

impl VaultStore {
    /// The target of a flip from the current state to `next` (same VK
    /// generation — rotations commit through the journal instead).
    pub fn flip_target(&self, mut next: Header) -> Header {
        next.manifest_generation = self.manifest.manifest_generation + 1;
        next
    }

    /// The manifest that describes `h` over the current DB.
    fn manifest_for(&self, h: &Header) -> Result<Manifest, ErrorCode> {
        let mut m = self.manifest.clone();
        m.manifest_generation = h.manifest_generation;
        m.registry_head = h.registry_head;
        m.vk_generation = h.vk_generation;
        m.objects = revision_rows::all_rows(&self.conn)?
            .iter()
            .map(|r| ManifestObject { record_id: r.record_id.clone(), revision_id: hex::encode(r.revision_id) })
            .collect();
        m.item_count = revision_rows::live_count(&self.conn)?;
        Ok(m)
    }

    /// Flip to `next` (its `manifest_generation` is set here).
    pub fn flip(&mut self, next: Header) -> Result<(), ErrorCode> {
        let h = self.flip_target(next);
        if kv::get::<Header>(&self.conn, KEY)?.as_ref() != Some(&h) {
            stamp(&self.conn, &h)?;
        }
        let m = self.manifest_for(&h)?;
        write_atomic(&self.dir.join(VAULT_HEADER_NAME), &header::write_header(&h)?)?;
        write_atomic(&self.dir.join(MANIFEST_NAME), &manifest::write_manifest(&m)?)?;
        self.header = h;
        self.manifest = m;
        Ok(())
    }

    /// Flip after a committed mutation, keeping the header's other fields.
    pub fn persist_head(&mut self) -> Result<(), ErrorCode> {
        self.flip(self.header.clone())
    }

    /// Move both heads to a new registry head (an appended enroll/revoke
    /// entry, §4), in lockstep so §3.5 still catches swapped files.
    pub fn set_registry_head(&mut self, head: [u8; 32]) -> Result<(), ErrorCode> {
        let mut h = self.header.clone();
        h.registry_head = super::header::Hex32(head);
        self.flip(h)
    }

    /// `open`'s recovery of an interrupted flip: only to the recorded
    /// target exactly one step past the manifest on disk, same vault and
    /// VK generation, with the header at either end of that step. The
    /// candidate pair is checked before anything is written.
    pub(super) fn roll_forward(&mut self) -> Result<bool, ErrorCode> {
        let Some(target) = kv::get::<Header>(&self.conn, KEY)? else {
            return Ok(false);
        };
        let (m, h) = (&self.manifest, &self.header);
        let one_step = target.manifest_generation == m.manifest_generation + 1;
        let same = target.vault_id == m.vault_id && target.vault_id == h.vault_id && target.vk_generation == m.vk_generation;
        let header_at_an_end = h.manifest_generation == m.manifest_generation || *h == target;
        if !(one_step && same && header_at_an_end) {
            return Ok(false);
        }
        let candidate = self.manifest_for(&target)?;
        check_against_header(&candidate, &target)?;
        let (old_h, old_m) = (std::mem::replace(&mut self.header, target), std::mem::replace(&mut self.manifest, candidate));
        if self.verify_objects().is_err() {
            self.header = old_h;
            self.manifest = old_m;
            return Ok(false);
        }
        write_atomic(&self.dir.join(VAULT_HEADER_NAME), &header::write_header(&self.header)?)?;
        write_atomic(&self.dir.join(MANIFEST_NAME), &manifest::write_manifest(&self.manifest)?)?;
        Ok(true)
    }
}
