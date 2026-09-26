//! Record backup objects (spec v0.4 §3.7): the byte-exact, self-describing
//! `OV0OBJ02` encoding of one revision. The object carries its own
//! `record_id` and `revision_id`; its storage name is
//! `blob_hash = SHA-256(bytes)` (§11.2). A VK rotation re-seals a revision
//! and changes its bytes and blob hash, never its identity.

use sha2::{Digest, Sha256};

use crate::errors::ErrorCode;
use crate::rev::{self as revisions, RevisionRow, MAX_PARENTS};

const MAGIC: &[u8; 8] = b"OV0OBJ02";
const RETIRED_MAGIC: &[u8; 8] = b"OV0OBJ01";
pub const MAX_OBJECT: usize = 1 << 20;

/// §11.2: an object's storage name.
pub fn blob_hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub fn encode(row: &RevisionRow) -> Result<Vec<u8>, ErrorCode> {
    if !revisions::parents_canonical(&row.parent_ids) {
        return Err(ErrorCode::InvalidInput);
    }
    let author = revisions::uuid_bytes(&row.author_device).ok_or(ErrorCode::DbCorrupt)?;
    let rid = revisions::uuid_bytes(&row.record_id).ok_or(ErrorCode::DbCorrupt)?;
    let mut o = Vec::with_capacity(128 + row.ct.len() + row.meta_ct.len());
    o.extend_from_slice(MAGIC);
    o.push(row.kind_tag);
    o.push(u8::from(row.deleted)); // flags bit0: tombstone; bits 1–7 reserved
    o.extend_from_slice(&row.vk_generation.to_be_bytes());
    o.extend_from_slice(&row.counter.to_be_bytes());
    o.extend_from_slice(&author);
    o.extend_from_slice(&rid);
    o.extend_from_slice(&row.revision_id);
    o.push(row.parent_ids.len() as u8);
    for p in &row.parent_ids {
        o.extend_from_slice(p);
    }
    o.extend_from_slice(&row.nonce);
    o.extend_from_slice(&(row.ct.len() as u32).to_be_bytes());
    o.extend_from_slice(&row.ct);
    o.extend_from_slice(&row.meta_nonce);
    o.extend_from_slice(&(row.meta_ct.len() as u32).to_be_bytes());
    o.extend_from_slice(&row.meta_ct);
    // Trailer (20 bytes): schema_version u32, created_at u64, updated_at u64.
    o.extend_from_slice(&row.schema_version.to_be_bytes());
    o.extend_from_slice(&row.created_at.to_be_bytes());
    o.extend_from_slice(&row.updated_at.to_be_bytes());
    if o.len() > MAX_OBJECT {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(o)
}

struct Cur<'a>(&'a [u8]);

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ErrorCode> {
        if self.0.len() < n {
            return Err(ErrorCode::FormatInvalid);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn arr<const N: usize>(&mut self) -> Result<[u8; N], ErrorCode> {
        Ok(self.take(N)?.try_into().expect("exact length"))
    }
}

/// Strict parse (§3.7): magic, reserved flag bits, `parent_count ≤ 8`,
/// sorted unique parents, length consistency, no trailing bytes.
pub fn decode(bytes: &[u8]) -> Result<RevisionRow, ErrorCode> {
    if bytes.len() > MAX_OBJECT {
        return Err(ErrorCode::FormatInvalid);
    }
    let mut c = Cur(bytes);
    let magic = c.take(8)?;
    if magic == RETIRED_MAGIC {
        return Err(ErrorCode::FormatInvalid);
    }
    if magic != MAGIC {
        return Err(ErrorCode::FormatTooNew);
    }
    let kind_tag = c.arr::<1>()?[0];
    let flags = c.arr::<1>()?[0];
    if flags > 1 || kind_tag == 0 {
        return Err(ErrorCode::FormatInvalid);
    }
    if !revisions::KNOWN_KINDS.contains(&kind_tag) {
        return Err(ErrorCode::FormatTooNew);
    }
    let vk_generation = u32::from_be_bytes(c.arr()?);
    let counter = u64::from_be_bytes(c.arr()?);
    let author: [u8; 16] = c.arr()?;
    // Counters live in SQLite INTEGER columns (SEC-I2); the author is
    // never all-zero (§3.7).
    if counter > i64::MAX as u64 || author == [0u8; 16] {
        return Err(ErrorCode::FormatInvalid);
    }
    let rid: [u8; 16] = c.arr()?;
    let revision_id: [u8; 32] = c.arr()?;
    let n = c.arr::<1>()?[0] as usize;
    if n > MAX_PARENTS {
        return Err(ErrorCode::FormatInvalid);
    }
    let mut parents = Vec::with_capacity(n);
    for _ in 0..n {
        parents.push(c.arr::<32>()?);
    }
    if !revisions::parents_canonical(&parents) {
        return Err(ErrorCode::FormatInvalid);
    }
    let nonce: [u8; 24] = c.arr()?;
    let ct_len = u32::from_be_bytes(c.arr()?) as usize;
    let ct = c.take(ct_len)?.to_vec();
    let meta_nonce: [u8; 24] = c.arr()?;
    let meta_len = u32::from_be_bytes(c.arr()?) as usize;
    let meta_ct = c.take(meta_len)?.to_vec();
    let schema_version = u32::from_be_bytes(c.arr()?);
    let created_at = u64::from_be_bytes(c.arr()?);
    let updated_at = u64::from_be_bytes(c.arr()?);
    if !c.0.is_empty() {
        return Err(ErrorCode::FormatInvalid);
    }
    Ok(RevisionRow {
        revision_id,
        record_id: revisions::uuid_string(&rid),
        parent_ids: parents,
        author_device: revisions::uuid_string(&author),
        counter,
        deleted: flags == 1,
        kind_tag,
        vk_generation,
        schema_version,
        nonce,
        ct,
        meta_nonce,
        meta_ct,
        created_at,
        updated_at,
    })
}

/// Decode a blob fetched under `expected_hash` and check that its embedded
/// ids match the index line that named it (§3.7).
pub fn decode_named(
    bytes: &[u8],
    expected_hash: &[u8; 32],
    record_id: &str,
    revision_id: &[u8; 32],
) -> Result<RevisionRow, ErrorCode> {
    if &blob_hash(bytes) != expected_hash {
        return Err(ErrorCode::BackupObjectMissing);
    }
    let row = decode(bytes)?;
    if row.record_id != record_id || &row.revision_id != revision_id {
        return Err(ErrorCode::ManifestMismatch);
    }
    Ok(row)
}
