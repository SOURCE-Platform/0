//! Peer operation bodies (wire annex revision 3, A.1 and A.3). Every body
//! is a §4.2 Document: a header entry, then list items, one entry each.
//! Strict decoding: an entry may hold only its allowed tags, none empty;
//! re-encoding must reproduce the input. Any violation is
//! `FORMAT_INVALID` (status 4).

use sha2::{Digest, Sha256};

use crate::crypto::tlv::{decode_document, encode_document, EntryBuilder, EntryReader};
use crate::errors::ErrorCode;

pub const BUCKETS: usize = 256;
pub const DIGEST_LEN: usize = BUCKETS * 32;
pub const MAX_HEADS: usize = 64;

/// Unavailable-item reasons (A.3).
pub const TOO_LARGE: u8 = 1;
pub const NOT_HELD: u8 = 2;
pub const WITHHELD: u8 = 3;
pub const TOO_MANY_HEADS: u8 = 4;

fn bad() -> ErrorCode {
    ErrorCode::FormatInvalid
}

/// One entry from `(tag, value)` pairs in ascending tag order.
pub fn entry(fields: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut b = EntryBuilder::new();
    for (t, v) in fields {
        b = b.field_bytes(*t, v).expect("ascending tags");
    }
    b.build()
}

pub fn uint(v: u64) -> Vec<u8> {
    crate::crypto::tlv::encode_uint(v)
}

/// The empty body: one empty header entry (`00 00000001 FF`).
pub fn empty() -> Vec<u8> {
    encode_document(&[entry(&[])])
}

/// A decoded document: its entries, each checked against allowed tags.
pub struct Doc {
    raw: Vec<Vec<u8>>,
}

impl Doc {
    pub fn parse(bytes: &[u8]) -> Result<Doc, ErrorCode> {
        let raw = decode_document(bytes).map_err(|_| bad())?;
        if raw.is_empty() || encode_document(&raw) != bytes {
            return Err(bad());
        }
        Ok(Doc { raw })
    }

    pub fn len(&self) -> usize {
        self.raw.len()
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Entry `i`, restricted to `allowed` tags, none empty.
    pub fn entry(&self, i: usize, allowed: &[u8]) -> Result<Fields<'_>, ErrorCode> {
        let r = EntryReader::parse(&self.raw[i]).map_err(|_| bad())?;
        let tags: Vec<u8> = r.tags().collect();
        if tags.iter().any(|t| !allowed.contains(t) || r.get(*t).is_none_or(|v| v.is_empty())) {
            return Err(bad());
        }
        Ok(Fields { r })
    }
}

pub struct Fields<'a> {
    r: EntryReader<'a>,
}

impl<'a> Fields<'a> {
    pub fn has(&self, t: u8) -> bool {
        self.r.get(t).is_some()
    }
    pub fn bytes(&self, t: u8) -> Result<&'a [u8], ErrorCode> {
        self.r.get(t).ok_or_else(bad)
    }
    pub fn opt(&self, t: u8) -> Option<&'a [u8]> {
        self.r.get(t)
    }
    pub fn fixed<const N: usize>(&self, t: u8) -> Result<[u8; N], ErrorCode> {
        self.bytes(t)?.try_into().map_err(|_| bad())
    }
    pub fn uint(&self, t: u8) -> Result<u64, ErrorCode> {
        self.r.get_uint(t).map_err(|_| bad())?.ok_or_else(bad)
    }
    pub fn flag(&self, t: u8) -> Result<bool, ErrorCode> {
        match self.uint(t)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(bad()),
        }
    }
    /// Concatenated 32-byte ids, ascending and distinct; `max` of them.
    pub fn ids(&self, t: u8, max: usize) -> Result<Vec<[u8; 32]>, ErrorCode> {
        ids_from(self.bytes(t)?, max)
    }
}

pub fn ids_from(b: &[u8], max: usize) -> Result<Vec<[u8; 32]>, ErrorCode> {
    if b.is_empty() || b.len() % 32 != 0 || b.len() / 32 > max {
        return Err(bad());
    }
    let ids: Vec<[u8; 32]> = b.chunks(32).map(|c| c.try_into().expect("32")).collect();
    if ids.windows(2).any(|w| w[0] >= w[1]) {
        return Err(bad());
    }
    Ok(ids)
}

pub fn concat(ids: &[[u8; 32]]) -> Vec<u8> {
    ids.iter().flatten().copied().collect()
}

/// The bucket of a record: the first byte of SHA-256(record_id).
pub fn bucket(record_id: &[u8; 16]) -> u8 {
    Sha256::digest(record_id)[0]
}

/// §22.8 heads digest over the servable heads: 256 buckets; a bucket's
/// digest is SHA-256 over, per record in ascending `record_id`,
/// `record_id ‖ u16be head_count ‖ heads ascending` (an empty bucket is
/// SHA-256 of nothing). `records` need not be sorted.
pub fn heads_digest(records: &[([u8; 16], Vec<[u8; 32]>)]) -> Vec<u8> {
    let mut sorted: Vec<&([u8; 16], Vec<[u8; 32]>)> = records.iter().collect();
    sorted.sort_by_key(|r| r.0);
    let mut hashers: Vec<Sha256> = (0..BUCKETS).map(|_| Sha256::new()).collect();
    for (id, heads) in sorted {
        let mut hs = heads.clone();
        hs.sort();
        let h = &mut hashers[bucket(id) as usize];
        h.update(id);
        h.update((hs.len() as u16).to_be_bytes());
        for x in &hs {
            h.update(x);
        }
    }
    hashers.into_iter().flat_map(|h| <[u8; 32]>::from(h.finalize())).collect()
}

/// A.3.1 `peer_hello`, both directions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub registry_seq: u64,
    pub registry_head: [u8; 32],
    pub committed_generation: u64,
    pub committed_manifest_hash: [u8; 32],
    pub heads_digest: Vec<u8>,
}

impl Hello {
    pub fn encode(&self) -> Vec<u8> {
        encode_document(&[entry(&[
            (0x01, uint(self.registry_seq)),
            (0x02, self.registry_head.to_vec()),
            (0x03, uint(self.committed_generation)),
            (0x04, self.committed_manifest_hash.to_vec()),
            (0x05, self.heads_digest.clone()),
        ])])
    }

    pub fn decode(bytes: &[u8]) -> Result<Hello, ErrorCode> {
        let d = Doc::parse(bytes)?;
        if d.len() != 1 {
            return Err(bad());
        }
        let e = d.entry(0, &[1, 2, 3, 4, 5])?;
        let h = Hello {
            registry_seq: e.uint(0x01)?,
            registry_head: e.fixed(0x02)?,
            committed_generation: e.uint(0x03)?,
            committed_manifest_hash: e.fixed(0x04)?,
            heads_digest: e.bytes(0x05)?.to_vec(),
        };
        if h.heads_digest.len() != DIGEST_LEN || h.encode() != bytes {
            return Err(bad());
        }
        Ok(h)
    }
}

/// A.3.6 `peer_status` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub vault_id: [u8; 16],
    pub registry: Vec<u8>,
    pub committed_seq: u64,
    pub committed_generation: u64,
    pub committed_manifest_hash: [u8; 32],
}

impl Status {
    pub fn encode(&self) -> Vec<u8> {
        encode_document(&[entry(&[
            (0x01, self.vault_id.to_vec()),
            (0x02, self.registry.clone()),
            (0x03, uint(self.committed_seq)),
            (0x04, uint(self.committed_generation)),
            (0x05, self.committed_manifest_hash.to_vec()),
        ])])
    }

    pub fn decode(bytes: &[u8]) -> Result<Status, ErrorCode> {
        let d = Doc::parse(bytes)?;
        if d.len() != 1 {
            return Err(bad());
        }
        let e = d.entry(0, &[1, 2, 3, 4, 5])?;
        let s = Status {
            vault_id: e.fixed(0x01)?,
            registry: e.bytes(0x02)?.to_vec(),
            committed_seq: e.uint(0x03)?,
            committed_generation: e.uint(0x04)?,
            committed_manifest_hash: e.fixed(0x05)?,
        };
        if s.encode() != bytes {
            return Err(bad());
        }
        Ok(s)
    }
}

/// A.3.5 `peer_revs_put` response counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PutCounts {
    pub admitted: u64,
    pub waiting: u64,
    pub refused: u64,
}

impl PutCounts {
    pub fn encode(&self) -> Vec<u8> {
        encode_document(&[entry(&[(0x01, uint(self.admitted)), (0x02, uint(self.waiting)), (0x03, uint(self.refused))])])
    }

    pub fn decode(bytes: &[u8]) -> Result<PutCounts, ErrorCode> {
        let d = Doc::parse(bytes)?;
        if d.len() != 1 {
            return Err(bad());
        }
        let e = d.entry(0, &[1, 2, 3])?;
        let c = PutCounts { admitted: e.uint(0x01)?, waiting: e.uint(0x02)?, refused: e.uint(0x03)? };
        if c.encode() != bytes {
            return Err(bad());
        }
        Ok(c)
    }
}
