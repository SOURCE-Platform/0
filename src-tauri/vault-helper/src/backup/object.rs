//! Record backup objects (spec §3.7): byte-exact `OV0OBJ01` encoding of
//! one revision, plus the record-bound fields the format leaves implicit
//! (record_id / rev_hash come from the object key; schema_version and
//! timestamps travel in a fixed trailer — documented Phase D deviation:
//! §3.7 omits them, but a restored revision needs them to reopen its
//! AEAD (schema_version is in the record AAD).

use crate::errors::ErrorCode;
use crate::storage::revisions::{self, RevisionRow};

const MAGIC: &[u8; 8] = b"OV0OBJ01";
const MAX_OBJECT: usize = 1 << 20;
const MAX_PARENTS: usize = 8;

pub fn key(row: &RevisionRow) -> String {
    format!("objects/rec/{}/{}", row.record_id, crate::crypto::hex::encode(row.rev_hash))
}

pub fn encode(row: &RevisionRow) -> Result<Vec<u8>, ErrorCode> {
    if row.parent_revs.len() > MAX_PARENTS {
        return Err(ErrorCode::InvalidInput);
    }
    let dev = revisions::uuid_bytes(&row.author_device).ok_or(ErrorCode::DbCorrupt)?;
    let mut o = Vec::with_capacity(64 + row.ct.len() + row.meta_ct.len());
    o.extend_from_slice(MAGIC);
    o.push(row.kind_tag);
    o.push(u8::from(row.deleted)); // flags bit0: tombstone (reserved field, §3.7)
    o.extend_from_slice(&row.vk_generation.to_be_bytes());
    o.extend_from_slice(&row.counter.to_be_bytes());
    o.extend_from_slice(&dev);
    o.push(row.parent_revs.len() as u8);
    for p in &row.parent_revs {
        o.extend_from_slice(p);
    }
    o.extend_from_slice(&row.nonce);
    o.extend_from_slice(&(row.ct.len() as u32).to_be_bytes());
    o.extend_from_slice(&row.ct);
    o.extend_from_slice(&row.meta_nonce);
    o.extend_from_slice(&(row.meta_ct.len() as u32).to_be_bytes());
    o.extend_from_slice(&row.meta_ct);
    // Trailer: schema_version u32, created_at u64, updated_at u64.
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
            return Err(ErrorCode::BackupObjectMissing);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn arr<const N: usize>(&mut self) -> Result<[u8; N], ErrorCode> {
        Ok(self.take(N)?.try_into().expect("exact length"))
    }
}

/// Strict parse; the rev_hash is recomputed from content and must match
/// the key's hash (content addressing, §3.2).
pub fn decode(record_id: &str, rev_hash: &[u8; 32], bytes: &[u8]) -> Result<RevisionRow, ErrorCode> {
    if bytes.len() > MAX_OBJECT {
        return Err(ErrorCode::BackupObjectMissing);
    }
    let mut c = Cur(bytes);
    if c.take(8)? != MAGIC {
        return Err(ErrorCode::FormatTooNew);
    }
    let kind_tag = c.arr::<1>()?[0];
    let flags = c.arr::<1>()?[0];
    if flags > 1 {
        return Err(ErrorCode::BackupObjectMissing);
    }
    let vk_generation = u32::from_be_bytes(c.arr()?);
    let counter = u64::from_be_bytes(c.arr()?);
    let dev: [u8; 16] = c.arr()?;
    let n = c.arr::<1>()?[0] as usize;
    if n > MAX_PARENTS {
        return Err(ErrorCode::BackupObjectMissing);
    }
    let mut parents = Vec::with_capacity(n);
    for _ in 0..n {
        parents.push(c.arr::<32>()?);
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
        return Err(ErrorCode::BackupObjectMissing); // no trailing bytes
    }
    let rid = revisions::uuid_bytes(record_id).ok_or(ErrorCode::BackupObjectMissing)?;
    let deleted = flags == 1;
    let computed = revisions::rev_hash(&rid, &parents, &dev, counter, deleted, &ct, &meta_ct);
    if &computed != rev_hash {
        return Err(ErrorCode::BackupObjectMissing);
    }
    Ok(RevisionRow {
        rev_hash: computed,
        record_id: record_id.to_string(),
        parent_revs: parents,
        author_device: uuid_string(&dev),
        counter,
        deleted,
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

pub fn uuid_string(b: &[u8; 16]) -> String {
    let h = crate::crypto::hex::encode(b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// `objects/rec/<record_id>/<rev_hash hex>` → (record_id, rev_hash).
pub fn parse_key(key: &str) -> Result<(String, [u8; 32]), ErrorCode> {
    let rest = key.strip_prefix("objects/rec/").ok_or(ErrorCode::BackupObjectMissing)?;
    let (rid, hash) = rest.split_once('/').ok_or(ErrorCode::BackupObjectMissing)?;
    let h = crate::crypto::hex::decode_array::<32>(hash).ok_or(ErrorCode::BackupObjectMissing)?;
    Ok((rid.to_string(), h))
}
