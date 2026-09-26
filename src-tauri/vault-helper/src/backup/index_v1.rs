//! Object index (spec §11.2): itself an object (`objects/index/<gen>`),
//! plaintext-classified fields only. Beyond the record objects it names
//! the header, registry, and wrap objects of the snapshot, so the signed
//! manifest's `object_index_hash` authenticates all of them. (The spec's
//! layout lists wraps/registry as fixed paths and omits the header; the
//! header carries the meta/import/locator salts a recovering device needs
//! — documented Phase D deviation.)

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use vault_proto::crypto::hex;
use vault_proto::errors::ErrorCode;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRef {
    pub key: String,
    /// hex SHA-256 of the object bytes
    pub sha256: String,
    pub size: u64,
}

impl IndexRef {
    pub fn of(key: String, bytes: &[u8]) -> IndexRef {
        IndexRef { key, sha256: hex::encode(Sha256::digest(bytes)), size: bytes.len() as u64 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectIndex {
    pub generation: u64,
    /// §3.4 accepted provider metadata (FR-01 shows it before recovery).
    pub item_count: u64,
    pub header: IndexRef,
    pub registry: IndexRef,
    pub wrap_mp: IndexRef,
    pub wrap_rk: Option<IndexRef>,
    pub records: Vec<IndexRef>,
}

impl ObjectIndex {
    pub fn all_refs(&self) -> Vec<&IndexRef> {
        let mut v = vec![&self.header, &self.registry, &self.wrap_mp];
        v.extend(self.wrap_rk.iter());
        v.extend(self.records.iter());
        v
    }

    /// SHA-256 over sorted `key sha256 size` lines (§11.2).
    pub fn hash(&self) -> [u8; 32] {
        let mut lines: Vec<String> = self
            .all_refs()
            .iter()
            .map(|r| format!("{} {} {}\n", r.key, r.sha256, r.size))
            .collect();
        lines.push(format!("#generation {}\n#items {}\n", self.generation, self.item_count));
        lines.sort();
        Sha256::digest(lines.concat().as_bytes()).into()
    }

    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("index serializes")
    }

    pub fn decode(bytes: &[u8]) -> Result<ObjectIndex, ErrorCode> {
        serde_json::from_slice(bytes).map_err(|_| ErrorCode::ManifestMismatch)
    }

    pub fn key(generation: u64) -> String {
        format!("objects/index/{generation}")
    }
}

/// Content-addressed key for a non-record object (header/registry/wrap).
pub fn meta_key(kind: &str, bytes: &[u8]) -> String {
    format!("objects/{kind}/{}", hex::encode(Sha256::digest(bytes)))
}

/// Verify downloaded bytes against their index entry.
pub fn check(r: &IndexRef, bytes: &[u8]) -> Result<(), ErrorCode> {
    if IndexRef::of(r.key.clone(), bytes) == *r {
        Ok(())
    } else {
        Err(ErrorCode::BackupObjectMissing)
    }
}
