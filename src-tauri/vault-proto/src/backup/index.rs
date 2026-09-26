//! Object index v2 (spec v0.4 §11.2). Canonical text; the exact bytes are
//! hashed, and that hash is both the manifest's `object_index_hash` and
//! the index's blob address.
//!
//! ```text
//! #ov0-index v2
//! #generation <u64>
//! #items <u64>
//! <role> <logical-id> <blob-sha256-hex> <size> [<parents>]   # sorted bytewise
//!   header   -
//!   registry -
//!   wrap     mp|rk
//!   env      <device_id hex32>
//!   rev      <record_id hex32>/<revision_id hex64>  <parents, comma-separated, sorted | "-">
//! ```
//!
//! Decoding is strict: re-encoding must reproduce the input. Structural
//! rules the provider enforces (§11.3 step 5) are in `check_structure`.

use std::collections::{BTreeSet, HashSet};

use sha2::{Digest, Sha256};

use crate::crypto::hex;
use crate::errors::ErrorCode;
use crate::rev::MAX_PARENTS;

const MAGIC: &str = "#ov0-index v2";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Header,
    Registry,
    WrapMp,
    WrapRk,
    Env { device_id: [u8; 16] },
    Rev { record_id: [u8; 16], revision_id: [u8; 32], parents: Vec<[u8; 32]> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub role: Role,
    pub blob: [u8; 32],
    pub size: u64,
}

impl IndexEntry {
    pub fn of(role: Role, bytes: &[u8]) -> IndexEntry {
        IndexEntry { role, blob: Sha256::digest(bytes).into(), size: bytes.len() as u64 }
    }

    fn line(&self) -> String {
        let (role, id, parents) = match &self.role {
            Role::Header => ("header", "-".to_string(), None),
            Role::Registry => ("registry", "-".to_string(), None),
            Role::WrapMp => ("wrap", "mp".to_string(), None),
            Role::WrapRk => ("wrap", "rk".to_string(), None),
            Role::Env { device_id } => ("env", hex::encode(device_id), None),
            Role::Rev { record_id, revision_id, parents } => {
                let p = if parents.is_empty() {
                    "-".to_string()
                } else {
                    parents.iter().map(hex::encode).collect::<Vec<_>>().join(",")
                };
                ("rev", format!("{}/{}", hex::encode(record_id), hex::encode(revision_id)), Some(p))
            }
        };
        let mut l = format!("{role} {id} {} {}", hex::encode(self.blob), self.size);
        if let Some(p) = parents {
            l.push(' ');
            l.push_str(&p);
        }
        l.push('\n');
        l
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectIndex {
    pub generation: u64,
    /// Live (non-deleted) record count — FR-01 shows it before recovery.
    pub item_count: u64,
    pub entries: Vec<IndexEntry>,
}

impl ObjectIndex {
    pub fn encode(&self) -> Vec<u8> {
        let mut lines: Vec<String> = self.entries.iter().map(IndexEntry::line).collect();
        lines.sort();
        let mut out = format!("{MAGIC}\n#generation {}\n#items {}\n", self.generation, self.item_count);
        for l in lines {
            out.push_str(&l);
        }
        out.into_bytes()
    }

    pub fn hash(&self) -> [u8; 32] {
        Sha256::digest(self.encode()).into()
    }

    /// Strict decode; entries come back in canonical (sorted) order.
    pub fn decode(bytes: &[u8]) -> Result<ObjectIndex, ErrorCode> {
        let bad = || ErrorCode::IndexInvalid;
        let text = std::str::from_utf8(bytes).map_err(|_| bad())?;
        let body = text.strip_suffix('\n').ok_or_else(bad)?;
        let mut lines = body.split('\n');
        match lines.next() {
            Some(MAGIC) => {}
            Some(l) if l.starts_with("#ov0-index v") => return Err(ErrorCode::FormatTooNew),
            _ => return Err(bad()),
        }
        let num = |l: Option<&str>, key: &str| -> Result<u64, ErrorCode> {
            let v = l.and_then(|l| l.strip_prefix(key)).ok_or_else(bad)?;
            parse_dec(v).ok_or_else(bad)
        };
        let generation = num(lines.next(), "#generation ")?;
        let item_count = num(lines.next(), "#items ")?;
        let entries = lines.map(parse_line).collect::<Result<Vec<_>, _>>()?;
        let idx = ObjectIndex { generation, item_count, entries };
        if idx.encode() != bytes {
            return Err(bad());
        }
        Ok(idx)
    }

    pub fn find(&self, role: &Role) -> Option<&IndexEntry> {
        self.entries.iter().find(|e| &e.role == role)
    }

    pub fn envs(&self) -> impl Iterator<Item = (&[u8; 16], &IndexEntry)> {
        self.entries.iter().filter_map(|e| match &e.role {
            Role::Env { device_id } => Some((device_id, e)),
            _ => None,
        })
    }

    pub fn revs(&self) -> impl Iterator<Item = &IndexEntry> {
        self.entries.iter().filter(|e| matches!(e.role, Role::Rev { .. }))
    }

    /// §11.3 step 5 structure (blob existence and the env-set rule are
    /// checked by the caller, which knows the store and the registry):
    /// exactly one header, registry and `wrap mp`, at most one `wrap rk`,
    /// no duplicate logical ids, and every `rev` parent listed (ancestor
    /// closure).
    pub fn check_structure(&self) -> Result<(), ErrorCode> {
        let bad = Err(ErrorCode::IndexInvalid);
        let count = |r: Role| self.entries.iter().filter(|e| e.role == r).count();
        if count(Role::Header) != 1 || count(Role::Registry) != 1 || count(Role::WrapMp) != 1 || count(Role::WrapRk) > 1 {
            return bad;
        }
        let mut envs = HashSet::new();
        let mut revs = HashSet::new();
        for e in &self.entries {
            match &e.role {
                Role::Env { device_id } if !envs.insert(*device_id) => return bad,
                Role::Rev { revision_id, .. } if !revs.insert(*revision_id) => return bad,
                _ => {}
            }
        }
        for e in self.revs() {
            if let Role::Rev { parents, .. } = &e.role {
                if parents.iter().any(|p| !revs.contains(p)) {
                    return bad;
                }
            }
        }
        Ok(())
    }

    /// Every distinct blob the index names.
    pub fn blobs(&self) -> BTreeSet<[u8; 32]> {
        self.entries.iter().map(|e| e.blob).collect()
    }
}

fn parse_dec(s: &str) -> Option<u64> {
    if s.is_empty() || (s.len() > 1 && s.starts_with('0')) || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn parse_line(line: &str) -> Result<IndexEntry, ErrorCode> {
    let bad = || ErrorCode::IndexInvalid;
    let f: Vec<&str> = line.split(' ').collect();
    let (blob, size) = match f.as_slice() {
        [_, _, b, s] | [_, _, b, s, _] => (hex::decode_array::<32>(b).ok_or_else(bad)?, parse_dec(s).ok_or_else(bad)?),
        _ => return Err(bad()),
    };
    let role = match (f[0], f[1], f.get(4)) {
        ("header", "-", None) => Role::Header,
        ("registry", "-", None) => Role::Registry,
        ("wrap", "mp", None) => Role::WrapMp,
        ("wrap", "rk", None) => Role::WrapRk,
        ("env", id, None) => Role::Env { device_id: hex::decode_array(id).ok_or_else(bad)? },
        ("rev", id, Some(p)) => {
            let (rid, rev) = id.split_once('/').ok_or_else(bad)?;
            let parents = if *p == "-" {
                Vec::new()
            } else {
                p.split(',').map(|h| hex::decode_array::<32>(h).ok_or_else(bad)).collect::<Result<Vec<_>, _>>()?
            };
            if parents.len() > MAX_PARENTS || !parents.windows(2).all(|w| w[0] < w[1]) {
                return Err(bad());
            }
            Role::Rev {
                record_id: hex::decode_array(rid).ok_or_else(bad)?,
                revision_id: hex::decode_array(rev).ok_or_else(bad)?,
                parents,
            }
        }
        _ => return Err(bad()),
    };
    Ok(IndexEntry { role, blob, size })
}
