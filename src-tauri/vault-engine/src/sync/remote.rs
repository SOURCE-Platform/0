//! A provider `state_get` response (spec v0.4 §11.3 routes), parsed
//! strictly. The helper never trusts the served `state_commit`: it
//! recomputes it from the manifest and checkpoint bytes it was given and
//! the served recovery-auth set, and refuses a mismatch (§11.2).

use serde_json::Value;
use sha2::{Digest, Sha256};
use vault_proto::b64;
use vault_proto::state::{recovery_auth_digest, state_commit, RecoveryAuthEntry};

use crate::backup::checkpoint::RegistryCheckpoint;
use crate::backup::manifest::SignedManifest;
use crate::crypto::recovery_auth::RecoveryClass;
use crate::errors::ErrorCode;
use crate::storage::header::strict_hex;

#[derive(Debug, Clone)]
pub struct RemoteState {
    pub generation: u64,
    pub state_commit: [u8; 32],
    pub manifest: SignedManifest,
    pub manifest_bytes: Vec<u8>,
    pub manifest_hash: [u8; 32],
    pub checkpoint: RegistryCheckpoint,
    pub checkpoint_bytes: Vec<u8>,
    pub vk_generation: u32,
    pub recovery_auth: Vec<RecoveryAuthEntry>,
}

pub fn parse(json: &[u8]) -> Result<RemoteState, ErrorCode> {
    let bad = || ErrorCode::ManifestMismatch;
    let v: Value = serde_json::from_slice(json).map_err(|_| bad())?;
    let text = |k: &str| v.get(k).and_then(Value::as_str).ok_or_else(bad);
    let manifest_bytes = b64::decode(text("manifest")?).ok_or_else(bad)?;
    let checkpoint_bytes = b64::decode(text("checkpoint")?).ok_or_else(bad)?;
    let manifest = SignedManifest::decode(&manifest_bytes)?;
    let checkpoint = RegistryCheckpoint::decode(&checkpoint_bytes)?;
    let mut recovery_auth = Vec::new();
    for e in v.get("recovery_auth").and_then(Value::as_array).ok_or_else(bad)? {
        let class = e.get("class").and_then(Value::as_u64).and_then(|c| RecoveryClass::from_code(c as u8)).ok_or_else(bad)?;
        let public = e.get("pub").and_then(Value::as_str).and_then(strict_hex::<65>).ok_or_else(bad)?;
        let salt = e.get("salt").and_then(Value::as_str).and_then(strict_hex::<16>).ok_or_else(bad)?;
        recovery_auth.push(RecoveryAuthEntry { class, public, salt });
    }
    let generation = v.get("generation").and_then(Value::as_u64).ok_or_else(bad)?;
    let vk_generation = v.get("vk_generation").and_then(Value::as_u64).ok_or_else(bad)?;
    let served = strict_hex::<32>(text("state_commit")?).ok_or_else(bad)?;
    let manifest_hash: [u8; 32] = Sha256::digest(&manifest_bytes).into();
    let checkpoint_hash: [u8; 32] = Sha256::digest(&checkpoint_bytes).into();
    let digest = recovery_auth_digest(&recovery_auth)?;
    let recomputed = state_commit(&manifest.vault_id, generation, &manifest_hash, &checkpoint_hash, &digest);
    if recomputed != served || generation != manifest.generation || vk_generation != u64::from(manifest.vk_generation) {
        return Err(bad());
    }
    Ok(RemoteState {
        generation,
        state_commit: recomputed,
        manifest,
        manifest_bytes,
        manifest_hash,
        checkpoint,
        checkpoint_bytes,
        vk_generation: vk_generation as u32,
        recovery_auth,
    })
}

/// The verified state re-encoded from its parsed fields only (review
/// SEC-O2 / VER-O3): what this Mac keeps and forwards to a peer under its
/// own signature carries nothing the provider added beside them. `parse`
/// of the result yields the same state.
pub fn canonical(r: &RemoteState) -> Vec<u8> {
    encode_fields(r.generation, r.vk_generation, &r.state_commit, &r.manifest_bytes, &r.checkpoint_bytes, &r.recovery_auth)
}

/// The annex A.3.2 encoding of a state's fields (shared with the XV-PEER
/// vectors, so the committed bytes are the shipped encoder's).
pub fn encode_fields(generation: u64, vk_generation: u32, state_commit: &[u8; 32], manifest: &[u8], checkpoint: &[u8], recovery_auth: &[RecoveryAuthEntry]) -> Vec<u8> {
    // Written out by hand: the annex A.3.2 key order must not depend on
    // whether `serde_json` was built with `preserve_order` (it is only in
    // this crate's dev-dependencies). Hex and base64 need no escaping.
    use crate::crypto::hex::encode as hex;
    let auth: Vec<String> = recovery_auth
        .iter()
        .map(|e| format!(r#"{{"class":{},"pub":"{}","salt":"{}"}}"#, e.class.code(), hex(e.public), hex(e.salt)))
        .collect();
    format!(
        r#"{{"generation":{},"vk_generation":{},"state_commit":"{}","manifest":"{}","checkpoint":"{}","recovery_auth":[{}]}}"#,
        generation,
        vk_generation,
        hex(state_commit),
        b64::encode(manifest),
        b64::encode(checkpoint),
        auth.join(",")
    )
    .into_bytes()
}
