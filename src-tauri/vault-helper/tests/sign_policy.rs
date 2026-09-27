//! The helper's checks before signing a provider request (spec v0.4
//! §11.4, PR-02/PR-05/PR-06, BK-25): only allowlisted origins; `blob_put`
//! only for the session's own blobs; `state_commit` only for the helper's
//! own staged body with its expected state; reads carry no body; a
//! recovery-class key never signs a publish it did not stage. Synthetic
//! keys only.

use std::collections::BTreeSet;

use vault_helper::crypto::recovery_auth::{derive, RecoveryClass};
use vault_helper::crypto::secret::SecretBytes;
use vault_helper::errors::ErrorCode;
use vault_helper::registry::device::{SoftwareDevice, PLATFORM_MACOS};
use vault_helper::sync::sign::{sign, Key, SignRequest, SignScope};
use vault_proto::request::{body_hash, Operation};

const ORIGIN: &str = "https://provider.test";
const VID: [u8; 16] = [0xa0; 16];

fn req(operation: Operation, blob: Option<[u8; 32]>, body: &[u8], expected: Option<[u8; 32]>) -> SignRequest {
    SignRequest { operation, blob, body_sha256: body_hash(body), expected_state: expected }
}

#[test]
fn signing_policy() {
    let dev = SoftwareDevice::generate("Synthetic Mac", PLATFORM_MACOS);
    let key = Key::Device(&dev);
    let put: BTreeSet<[u8; 32]> = [[1u8; 32]].into_iter().collect();
    let staged_body = b"staged transition body";
    let scope = SignScope { put_blobs: &put, staged: Some((body_hash(staged_body), [7; 32])) };
    let refused = Err(ErrorCode::SigningRefused);
    let s = |origin: &str, k: Key<'_>, r: &SignRequest, sc: &SignScope<'_>| sign(origin, VID, k, r, sc, 0, 1_900_000_000).map(|(_, h)| h);

    // BK-25: an origin outside the compiled-in allowlist.
    assert_eq!(s("https://phishing.test", Key::Device(&dev), &req(Operation::StateGet, None, b"", None), &scope), refused);
    // Reads: fine, but never with a body.
    assert!(s(ORIGIN, Key::Device(&dev), &req(Operation::StateGet, None, b"", None), &scope).is_ok());
    assert_eq!(s(ORIGIN, Key::Device(&dev), &req(Operation::BlobGet, Some([9; 32]), b"x", None), &scope), refused);
    // PR-06: blob_put only for the session's own blobs.
    assert!(s(ORIGIN, Key::Device(&dev), &req(Operation::BlobPut, Some([1; 32]), b"blob", None), &scope).is_ok());
    assert_eq!(s(ORIGIN, Key::Device(&dev), &req(Operation::BlobPut, Some([2; 32]), b"blob", None), &scope), refused);
    // PR-02/PR-04: state_commit only for the staged body and its expected state.
    assert!(s(ORIGIN, key, &req(Operation::StateCommit, None, staged_body, Some([7; 32])), &scope).is_ok());
    assert_eq!(s(ORIGIN, Key::Device(&dev), &req(Operation::StateCommit, None, b"caller-supplied body", Some([7; 32])), &scope), refused);
    assert_eq!(s(ORIGIN, Key::Device(&dev), &req(Operation::StateCommit, None, staged_body, Some([8; 32])), &scope), refused);
    // PR-05: a recovery key with nothing staged cannot sign any write.
    let rk = derive(RecoveryClass::Rk, &SecretBytes::new([0x52; 32]), &[0x53; 16], &VID).unwrap();
    let none = BTreeSet::new();
    let reads_only = SignScope { put_blobs: &none, staged: None };
    assert!(s(ORIGIN, Key::Recovery(&rk, RecoveryClass::Rk), &req(Operation::StateGet, None, b"", None), &reads_only).is_ok());
    assert_eq!(s(ORIGIN, Key::Recovery(&rk, RecoveryClass::Rk), &req(Operation::StateCommit, None, staged_body, Some([7; 32])), &reads_only), refused);
    assert_eq!(s(ORIGIN, Key::Recovery(&rk, RecoveryClass::Rk), &req(Operation::BlobPut, Some([1; 32]), b"blob", None), &reads_only), refused);
}
