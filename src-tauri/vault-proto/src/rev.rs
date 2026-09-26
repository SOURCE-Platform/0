//! Revision identity (spec v0.4 §3.2, §2.6): the `RevisionRow` a backup
//! object carries, its `graph_digest` and AAD binding, and id helpers.
//! Storage and merge live in the helper.

use sha2::{Digest, Sha256};

use crate::errors::ErrorCode;

/// The per-revision AAD binding for record and metadata ciphertexts
/// (§2.6): `revision_id` and `graph_digest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevBinding {
    pub revision_id: [u8; 32],
    pub graph_digest: [u8; 32],
}

/// Tag byte for `record_flags` evidence and `refused_revs` reasons.
pub const REFUSED_COUNTER_REGRESSION: u8 = 1;
pub const REFUSED_REVOKED_AUTHOR: u8 = 2;
pub const REFUSED_ZERO_AUTHOR: u8 = 3;
pub const REFUSED_MALFORMED: u8 = 4;
/// Sealed under a `vk_generation` other than the local one (SEC-B1).
pub const REFUSED_GENERATION: u8 = 5;

/// Record kinds this build understands (§8: login, card). An unknown
/// non-zero kind is a newer format (`FORMAT_TOO_NEW`).
pub const KNOWN_KINDS: [u8; 2] = [1, 2];

pub const MAX_PARENTS: usize = 8;

/// Largest accepted counter: counters live in SQLite INTEGER columns and
/// the next one must still fit (review SEC-I2, SEC-O7).
pub const MAX_COUNTER: u64 = (1 << 62) - 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionRow {
    pub revision_id: [u8; 32],
    pub record_id: String,
    /// Parent revision_ids, sorted ascending, unique.
    pub parent_ids: Vec<[u8; 32]>,
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

impl RevisionRow {
    /// The AAD binding for this revision's ciphertexts (§2.6).
    pub fn bind(&self) -> Result<RevBinding, ErrorCode> {
        let rid = uuid_bytes(&self.record_id).ok_or(ErrorCode::DbCorrupt)?;
        let author = uuid_bytes(&self.author_device).ok_or(ErrorCode::DbCorrupt)?;
        Ok(RevBinding {
            revision_id: self.revision_id,
            graph_digest: graph_digest(
                &rid,
                &self.revision_id,
                &author,
                self.counter,
                self.deleted,
                self.kind_tag,
                &self.parent_ids,
            ),
        })
    }

    /// Everything but the ciphertexts, nonces and `vk_generation`: two
    /// representations of one revision (before/after a rotation) agree here.
    pub fn same_graph(&self, other: &RevisionRow) -> bool {
        self.revision_id == other.revision_id
            && self.record_id == other.record_id
            && self.parent_ids == other.parent_ids
            && self.author_device == other.author_device
            && self.counter == other.counter
            && self.deleted == other.deleted
            && self.kind_tag == other.kind_tag
            && self.schema_version == other.schema_version
    }
}

/// §2.6: SHA-256("ov0/rev-graph/v2" ‖ record_id ‖ revision_id ‖ author ‖
/// u64be(counter) ‖ u8(flags) ‖ u8(kind) ‖ u8(n) ‖ parents(sorted)).
pub fn graph_digest(
    record_id: &[u8; 16],
    revision_id: &[u8; 32],
    author: &[u8; 16],
    counter: u64,
    deleted: bool,
    kind_tag: u8,
    parents: &[[u8; 32]],
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"ov0/rev-graph/v2");
    h.update(record_id);
    h.update(revision_id);
    h.update(author);
    h.update(counter.to_be_bytes());
    h.update([u8::from(deleted)]);
    h.update([kind_tag]);
    h.update([parents.len() as u8]);
    for p in parents {
        h.update(p);
    }
    h.finalize().into()
}

/// Parents must be sorted ascending, unique, and at most 8 (§3.7).
pub fn parents_canonical(parents: &[[u8; 32]]) -> bool {
    parents.len() <= MAX_PARENTS && parents.windows(2).all(|w| w[0] < w[1])
}

pub fn uuid_bytes(uuid: &str) -> Option<[u8; 16]> {
    let hex: String = uuid.chars().filter(|c| *c != '-').collect();
    crate::crypto::hex::decode_array(&hex)
}

pub fn uuid_string(b: &[u8; 16]) -> String {
    let h = crate::crypto::hex::encode(b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// Random UUIDv4 text (§3.3: content-independent).
pub fn new_record_id() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("OS CSPRNG failure is unrecoverable");
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // variant 10
    uuid_string(&b)
}

/// §3.2: 256 bits from the OS RNG, created once per authored revision.
pub fn new_revision_id() -> [u8; 32] {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("OS CSPRNG failure is unrecoverable");
    b
}

