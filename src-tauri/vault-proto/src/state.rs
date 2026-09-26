//! Vault-state commitment and `StateTransition` bodies (spec v0.4 §11.2,
//! §11.3).
//!
//! ```text
//! recovery_auth_digest = SHA-256("ov0/recovery-auth-set/v2" ‖ for c in [mp, rk] if present:
//!                                 u8(class) ‖ pub(65) ‖ salt(16))
//! state_commit = SHA-256("ov0/vault-state/v2" ‖ TLV{
//!     0x01 vault_id  0x02 generation  0x03 manifest_hash
//!     0x04 checkpoint_hash  0x05 recovery_auth_digest })
//! ```

use sha2::{Digest, Sha256};

use crate::crypto::ecdsa;
use crate::crypto::recovery_auth::{RecoveryClass, CLASS_MP, CLASS_RK};
use crate::crypto::tlv::{EntryBuilder, EntryReader};
use crate::errors::ErrorCode;

pub const PROTO: u32 = 2;
/// Inline bootstrap blobs of a `create` total at most 1 MiB (§11.2).
pub const MAX_BOOTSTRAP: usize = 1 << 20;
const AUTH_ENTRY_LEN: usize = 1 + 65 + 16;

/// One registered recovery-auth key: `class ‖ pub(65) ‖ salt(16)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryAuthEntry {
    pub class: RecoveryClass,
    pub public: [u8; 65],
    pub salt: [u8; 16],
}

impl RecoveryAuthEntry {
    fn bytes(&self) -> [u8; AUTH_ENTRY_LEN] {
        let mut b = [0u8; AUTH_ENTRY_LEN];
        b[0] = self.class.code();
        b[1..66].copy_from_slice(&self.public);
        b[66..].copy_from_slice(&self.salt);
        b
    }
}

/// Sorted by class, one per class; the canonical form for both the digest
/// and `recovery_auth_updates`.
fn canonical(entries: &[RecoveryAuthEntry]) -> Result<Vec<RecoveryAuthEntry>, ErrorCode> {
    let mut v = entries.to_vec();
    v.sort_by_key(|e| e.class);
    if v.windows(2).any(|w| w[0].class == w[1].class) {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(v)
}

pub fn recovery_auth_digest(entries: &[RecoveryAuthEntry]) -> Result<[u8; 32], ErrorCode> {
    let mut h = Sha256::new();
    h.update(b"ov0/recovery-auth-set/v2");
    for e in canonical(entries)? {
        h.update(e.bytes());
    }
    Ok(h.finalize().into())
}

pub fn state_commit(
    vault_id: &[u8; 16],
    generation: u64,
    manifest_hash: &[u8; 32],
    checkpoint_hash: &[u8; 32],
    recovery_auth_digest: &[u8; 32],
) -> [u8; 32] {
    let tlv = EntryBuilder::new()
        .field_bytes(0x01, vault_id)
        .and_then(|b| b.field_uint(0x02, generation))
        .and_then(|b| b.field_bytes(0x03, manifest_hash))
        .and_then(|b| b.field_bytes(0x04, checkpoint_hash))
        .and_then(|b| b.field_bytes(0x05, recovery_auth_digest))
        .expect("ascending tags")
        .build();
    let mut h = Sha256::new();
    h.update(b"ov0/vault-state/v2");
    h.update(tlv);
    h.finalize().into()
}

pub fn encode_auth_updates(entries: &[RecoveryAuthEntry]) -> Result<Vec<u8>, ErrorCode> {
    Ok(canonical(entries)?.iter().flat_map(|e| e.bytes()).collect())
}

/// Strict: whole 82-byte entries, classes strictly ascending, known
/// classes, valid uncompressed P-256 points.
pub fn decode_auth_updates(bytes: &[u8]) -> Result<Vec<RecoveryAuthEntry>, ErrorCode> {
    let bad = || ErrorCode::InvalidInput;
    if bytes.is_empty() || !bytes.len().is_multiple_of(AUTH_ENTRY_LEN) {
        return Err(bad());
    }
    let mut out: Vec<RecoveryAuthEntry> = Vec::new();
    for c in bytes.chunks(AUTH_ENTRY_LEN) {
        let class = RecoveryClass::from_code(c[0]).ok_or_else(bad)?;
        if out.last().is_some_and(|p| p.class >= class) {
            return Err(bad());
        }
        let public: [u8; 65] = c[1..66].try_into().map_err(|_| bad())?;
        ecdsa::parse_verifying_key(&public).map_err(|_| bad())?;
        out.push(RecoveryAuthEntry { class, public, salt: c[66..].try_into().map_err(|_| bad())? });
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    Create,
    Publish,
    Finalize,
}

impl TransitionKind {
    pub fn code(self) -> u8 {
        match self {
            TransitionKind::Create => 1,
            TransitionKind::Publish => 2,
            TransitionKind::Finalize => 3,
        }
    }
}

/// §11.3 `StateTransition` body (canonical TLV).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateTransition {
    pub vault_id: [u8; 16],
    pub kind: TransitionKind,
    /// All-zero iff `create`.
    pub expected_state: [u8; 32],
    pub manifest: Vec<u8>,
    pub checkpoint: Vec<u8>,
    pub recovery_auth_updates: Vec<RecoveryAuthEntry>,
    /// `create` only.
    pub handle_key: Option<[u8; 32]>,
    /// `create` only (required): every blob the genesis state references.
    pub bootstrap_blobs: Vec<Vec<u8>>,
}

impl StateTransition {
    pub fn encode(&self) -> Result<Vec<u8>, ErrorCode> {
        self.check_shape()?;
        let mut b = EntryBuilder::new()
            .field_uint(0x01, u64::from(PROTO))
            .and_then(|b| b.field_bytes(0x02, &self.vault_id))
            .and_then(|b| b.field_uint(0x03, u64::from(self.kind.code())))
            .and_then(|b| b.field_bytes(0x04, &self.expected_state))
            .and_then(|b| b.field_bytes(0x05, &self.manifest))
            .and_then(|b| b.field_bytes(0x06, &self.checkpoint))
            .map_err(|_| ErrorCode::Internal)?;
        if !self.recovery_auth_updates.is_empty() {
            b = b.field_bytes(0x07, &encode_auth_updates(&self.recovery_auth_updates)?).map_err(|_| ErrorCode::Internal)?;
        }
        if let Some(h) = &self.handle_key {
            b = b.field_bytes(0x08, h).map_err(|_| ErrorCode::Internal)?;
        }
        if self.kind == TransitionKind::Create {
            let mut blob = Vec::new();
            for x in &self.bootstrap_blobs {
                blob.extend_from_slice(&(x.len() as u32).to_be_bytes());
                blob.extend_from_slice(x);
            }
            b = b.field_bytes(0x09, &blob).map_err(|_| ErrorCode::Internal)?;
        }
        Ok(b.build())
    }

    fn check_shape(&self) -> Result<(), ErrorCode> {
        let create = self.kind == TransitionKind::Create;
        let total: usize = self.bootstrap_blobs.iter().map(Vec::len).sum();
        let ok = (self.expected_state == [0u8; 32]) == create
            && self.handle_key.is_some() == create
            && self.bootstrap_blobs.is_empty() != create
            && self.bootstrap_blobs.iter().all(|b| !b.is_empty());
        if !ok {
            return Err(ErrorCode::ManifestInvalid);
        }
        if total > MAX_BOOTSTRAP {
            return Err(ErrorCode::TooLarge);
        }
        Ok(())
    }

    /// Strict decode: canonical TLV, the kind's presence rules, and
    /// re-encoding reproduces the input.
    pub fn decode(bytes: &[u8]) -> Result<StateTransition, ErrorCode> {
        let bad = || ErrorCode::ManifestInvalid;
        let r = EntryReader::parse(bytes).map_err(|_| bad())?;
        if r.get_uint(0x01).map_err(|_| bad())? != Some(u64::from(PROTO)) {
            return Err(bad());
        }
        let kind = match r.get_uint(0x03).map_err(|_| bad())? {
            Some(1) => TransitionKind::Create,
            Some(2) => TransitionKind::Publish,
            Some(3) => TransitionKind::Finalize,
            _ => return Err(bad()),
        };
        let mut bootstrap_blobs = Vec::new();
        if let Some(mut rest) = r.get(0x09) {
            while !rest.is_empty() {
                let len = u32::from_be_bytes(rest.get(..4).ok_or_else(bad)?.try_into().map_err(|_| bad())?) as usize;
                let blob = rest.get(4..4 + len).ok_or_else(bad)?;
                bootstrap_blobs.push(blob.to_vec());
                rest = &rest[4 + len..];
            }
        }
        let t = StateTransition {
            vault_id: r.get(0x02).ok_or_else(bad)?.try_into().map_err(|_| bad())?,
            kind,
            expected_state: r.get(0x04).ok_or_else(bad)?.try_into().map_err(|_| bad())?,
            manifest: r.get(0x05).ok_or_else(bad)?.to_vec(),
            checkpoint: r.get(0x06).ok_or_else(bad)?.to_vec(),
            recovery_auth_updates: match r.get(0x07) {
                Some(u) => decode_auth_updates(u).map_err(|_| bad())?,
                None => Vec::new(),
            },
            handle_key: r.get(0x08).map(|h| h.try_into()).transpose().map_err(|_| bad())?,
            bootstrap_blobs,
        };
        if r.get(0x09).is_some() != (kind == TransitionKind::Create) || t.encode()? != bytes {
            return Err(bad());
        }
        Ok(t)
    }
}

pub fn class_code_ok(c: u8) -> bool {
    c == CLASS_MP || c == CLASS_RK
}
