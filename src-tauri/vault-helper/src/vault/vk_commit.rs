//! Vault-key commitment (spec §22.4 erratum, review VER-B3).
//!
//! Every file in the vault directory can be rewritten by a same-user
//! process, so no wrap or envelope on disk proves which key is the vault's:
//! anyone can seal a key of their choosing under a password they chose (or
//! to this device's public agreement key). Before a key from any unlock
//! path becomes resident, it must match a commitment signed by this
//! device's Secure Enclave signing key — which only the helper can use.
//! The public key is read from the Secure Enclave itself, never from the
//! device file on disk. The helper signs a commitment only for a key it
//! already trusts: a new vault, a rotation it performed, a verified
//! adoption, a completed recovery.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::crypto::secret::SecretBytes;
use crate::crypto::{ecdsa, hex, hkdf};
use crate::device::{se, SeDevice};
use crate::errors::ErrorCode;
use crate::storage::store::write_atomic;

pub const FILE: &str = "vk_commit.json";
const DOMAIN: &[u8] = b"ov0/vk-commit/v1";

#[derive(Serialize, Deserialize)]
struct Commit {
    v: u32,
    vk_generation: u32,
    signature: String,
}

fn digest(vault_id: &[u8; 16], generation: u32, vk: &SecretBytes<32>) -> Result<[u8; 32], ErrorCode> {
    let tag = hkdf::hkdf32(vk.expose(), &[], DOMAIN).map_err(|_| ErrorCode::Internal)?;
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(vault_id);
    h.update(generation.to_be_bytes());
    h.update(tag.expose());
    Ok(h.finalize().into())
}

/// The commitment file's bytes for a key the helper trusts, signed by the
/// SE key behind `tag` (low-S). Staged by every journal that changes the
/// key, so key and commitment commit together (review SEC-B1).
pub fn encode_with_tag(tag: &str, vault_id: &[u8; 16], generation: u32, vk: &SecretBytes<32>) -> Result<Vec<u8>, ErrorCode> {
    let d = digest(vault_id, generation, vk)?;
    let raw = se::sign_digest(tag, &d).map_err(|_| ErrorCode::KeychainUnavailable)?;
    let sig = ecdsa::normalize_low_s(&raw).map_err(|_| ErrorCode::Internal)?;
    ecdsa::verify_prehash(&se::signing_public(tag)?, &d, &sig).map_err(|_| ErrorCode::Internal)?;
    let body = Commit { v: 1, vk_generation: generation, signature: hex::encode(sig) };
    serde_json::to_vec(&body).map_err(|_| ErrorCode::Internal)
}

/// `encode_with_tag` for this vault directory's own device.
pub fn encode(dir: &Path, vault_id: &[u8; 16], generation: u32, vk: &SecretBytes<32>) -> Result<Vec<u8>, ErrorCode> {
    encode_with_tag(SeDevice::load(dir)?.key_tag(), vault_id, generation, vk)
}

/// Sign the commitment for a key the helper trusts (outside a journal:
/// a new vault, and the retry of a commitment that failed to sign).
pub fn record(dir: &Path, vault_id: &[u8; 16], generation: u32, vk: &SecretBytes<32>) -> Result<(), ErrorCode> {
    write_atomic(&dir.join(FILE), &encode(dir, vault_id, generation, vk)?)
}

/// How a key about to become resident relates to this device's identity.
pub enum Verdict {
    /// Matches the commitment this device's Secure Enclave signed.
    Committed,
    /// This device has no usable Secure Enclave identity (a moved or
    /// restored Mac, a wiped key): nothing can be verified, and nothing it
    /// does can carry authority either. Unlock is read-only (§2.8, §22.4).
    NoIdentity,
}

/// The key about to become resident is this vault's committed key at
/// this generation. A missing, stale or forged commitment is refused.
pub fn verify(dir: &Path, vault_id: &[u8; 16], generation: u32, vk: &SecretBytes<32>) -> Result<Verdict, ErrorCode> {
    let Some(public) = SeDevice::load(dir).ok().and_then(|d| se::signing_public(d.key_tag()).ok()) else {
        return Ok(Verdict::NoIdentity);
    };
    let body: Commit = std::fs::read(dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).ok_or(ErrorCode::WrongCredential)?;
    let sig = hex::decode(&body.signature).ok_or(ErrorCode::WrongCredential)?;
    if body.v != 1 || body.vk_generation != generation {
        return Err(ErrorCode::WrongCredential);
    }
    ecdsa::verify_prehash(&public, &digest(vault_id, generation, vk)?, &sig).map_err(|_| ErrorCode::WrongCredential)?;
    Ok(Verdict::Committed)
}

/// Ops a key without a verified commitment may serve: reads and status
/// only. Anything that authors, signs or changes authority is refused.
pub const UNVERIFIED_OPS: &[&str] = &[
    "list_items", "reveal", "list_history", "list_deleted", "list_devices", "quarantine_status",
    "remote_update_status", "set_auto_lock_minutes",
];

/// Record the commitment for the core's resident key; on failure mark it
/// pending so the next op retries while the key is still resident.
pub fn commit_resident(c: &mut super::VaultCore) {
    if c.unverified_key {
        return; // never vouch for a key nothing verified
    }
    let ok = match (c.store.as_ref(), c.vk.as_ref()) {
        (Some(s), Some(vk)) => record(&s.dir, &s.header.vault_id.0, s.header.vk_generation, vk).is_ok(),
        _ => false,
    };
    c.vk_commit_pending = !ok && c.vk.is_some();
}
