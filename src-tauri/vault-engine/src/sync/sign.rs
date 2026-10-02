//! Provider request signing (spec v0.4 §11.4). The helper builds every
//! canonical field itself — audience from its own allowlisted provider
//! origin, path and method from the operation, vault, time and nonce —
//! and applies the "helper checks before signing": main supplies only the
//! typed operation, typed parameters and the body hash, and cannot get a
//! signature over anything else (PR-02, PR-04, PR-05, PR-06).

use std::collections::BTreeSet;

use vault_proto::request::{auth_header, Operation, ProviderRequest, SignerId};

use crate::crypto::recovery_auth::{key_id, RecoveryAuthKey, RecoveryClass};
use crate::errors::ErrorCode;
use crate::registry::device::DeviceIdentity;
use crate::storage::header::check_provider;

/// Clock sanity (§11.4): `t` may run at most this far ahead of now.
const MAX_AHEAD: u64 = 60;

/// What a session allows to be signed (from the helper's own staging).
pub struct SignScope<'a> {
    /// `blob_put` only for these hashes.
    pub put_blobs: &'a BTreeSet<[u8; 32]>,
    /// `state_commit` only for this staged body and its expected state.
    pub staged: Option<([u8; 32], [u8; 32])>,
}

pub struct SignRequest {
    pub operation: Operation,
    pub blob: Option<[u8; 32]>,
    pub body_sha256: [u8; 32],
    pub expected_state: Option<[u8; 32]>,
}

pub enum Key<'a> {
    Device(&'a dyn DeviceIdentity),
    Recovery(&'a RecoveryAuthKey, RecoveryClass),
}

/// §11.4 checks, then build and sign. Returns the `Ov0-Auth` value.
#[allow(clippy::too_many_arguments)]
pub fn sign(
    origin: &str,
    vault_id: [u8; 16],
    key: Key<'_>,
    req: &SignRequest,
    scope: &SignScope<'_>,
    created_at: u64,
    now: u64,
) -> Result<(ProviderRequest, String), ErrorCode> {
    let refused = Err(ErrorCode::SigningRefused);
    check_provider(origin)?;
    if now < created_at || now > u64::MAX - MAX_AHEAD {
        return refused;
    }
    match req.operation {
        Operation::BlobPut if req.blob.is_none_or(|b| !scope.put_blobs.contains(&b)) => return refused,
        Operation::StateCommit => match scope.staged {
            Some((body, expected)) if body == req.body_sha256 && Some(expected) == req.expected_state => {}
            _ => return refused,
        },
        // Reads carry no body.
        Operation::StateGet | Operation::BlobGet if req.body_sha256 != vault_proto::request::body_hash(b"") => return refused,
        _ => {}
    }
    let mut n = [0u8; 16];
    getrandom::fill(&mut n).map_err(|_| ErrorCode::Internal)?;
    let signer = match &key {
        Key::Device(d) => SignerId::Device { device_id: d.device_id(), key_id: key_id(&d.sign_pub()) },
        Key::Recovery(k, c) => SignerId::Recovery { class: c.code(), key_id: k.key_id() },
    };
    let pr = ProviderRequest::build(origin, vault_id, req.operation, req.blob.as_ref(), signer, req.body_sha256, req.expected_state, now, n)?;
    let digest = pr.prehash();
    let sig = match key {
        // Background signing never prompts; an unavailable Keychain or
        // Enclave surfaces as KEYCHAIN_UNAVAILABLE (§1.6).
        Key::Device(d) => d.sign_prehash(&digest).map_err(|_| ErrorCode::KeychainUnavailable)?,
        Key::Recovery(k, _) => k.sign_prehash(&digest),
    };
    let header = auth_header(&pr.encode(), &sig);
    Ok((pr, header))
}
