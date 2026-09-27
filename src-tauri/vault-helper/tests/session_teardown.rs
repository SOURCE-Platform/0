//! Session teardown (spec v0.4 §1.3 TR-07, §13.3 ST-03): an abandoned
//! recovery — the app disconnecting, a lock, or `session_close` — leaves
//! RECOVERING for the state the vault directory implies, never a
//! vault-less LOCKED, and drops the recovery session; TR-08 a restart
//! sweeps an interrupted recovery; TR-09 a large enrollment bundle
//! streams. Synthetic only.

use serde_json::json;
use vault_helper::state::VaultState;
use vault_helper::vault::{LockReason, VaultCore};

fn tmp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("vh-teardown-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn abandoned_recovery_returns_to_uninitialized() {
    let dir = tmp("leave");
    let mut c = VaultCore::boot(dir.clone());
    assert_eq!(c.state, VaultState::Uninitialized);
    c.state = VaultState::Recovering;
    let ev = c.leave_recovery().expect("a state event");
    assert_eq!(c.state, VaultState::Uninitialized);
    assert_eq!(ev["state"], json!("uninitialized"));
    assert!(c.provider.recovery.is_none());
    // Not recovering: nothing changes, no event.
    assert!(c.leave_recovery().is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn lock_during_recovery_is_not_a_vaultless_locked() {
    let dir = tmp("lock");
    let mut c = VaultCore::boot(dir.clone());
    c.state = VaultState::Recovering;
    let events = c.lock(LockReason::Explicit);
    assert_eq!(c.state, VaultState::Uninitialized);
    assert_eq!(events.len(), 1, "only the state change back to uninitialized: {events:?}");
    std::fs::remove_dir_all(&dir).ok();
}

/// TR-08: a restart sweeps an interrupted recovery's staging directory.
#[test]
fn restart_sweeps_recovery_staging() {
    let dir = tmp("sweep");
    std::fs::create_dir_all(dir.join("recovered")).unwrap();
    std::fs::write(dir.join("recovered/vault.db"), b"synthetic partial staging").unwrap();
    let c = VaultCore::boot(dir.clone());
    assert_eq!(c.state, VaultState::Uninitialized);
    assert!(!dir.join("recovered").exists());
    std::fs::remove_dir_all(&dir).ok();
}

/// TR-09: an enrollment bundle larger than one frame is delivered as a
/// one-blob stream session: read in chunks, verified, closed; a lock drops
/// it with the enrollment.
#[test]
fn large_enrollment_bundle_streams() {
    use std::sync::{Arc, Mutex};
    use vault_helper::vault::enroll_ops::deliver_bundle;
    use vault_helper::vault::provider_ops::{session_close, stream_read};
    let dir = tmp("bundle");
    let core = Arc::new(Mutex::new(VaultCore::boot(dir.clone())));
    let small = json!({"objects": [["a", "b"]]});
    assert_eq!(deliver_bundle(&mut core.lock().unwrap(), small.clone()), json!({ "bundle": small }));
    let big = json!({"objects": (0..400).map(|i| vec![format!("{i:064x}"), "ab".repeat(100)]).collect::<Vec<_>>()});
    let out = deliver_bundle(&mut core.lock().unwrap(), big.clone());
    let (session, sha) = (out["session"].as_str().unwrap().to_string(), out["stream"].as_str().unwrap().to_string());
    assert!(out["size"].as_u64().unwrap() > 64 * 1024, "larger than a frame");
    let mut bytes = Vec::new();
    loop {
        let r = stream_read(&core, &json!({"session": session, "sha256": sha, "offset": bytes.len()})).response;
        bytes.extend(vault_proto::b64::decode(r["data"].as_str().unwrap()).unwrap());
        if r["eof"] == true {
            break;
        }
    }
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(), big);
    session_close(&core, &json!({"session": session}));
    let r = stream_read(&core, &json!({"session": session, "sha256": sha, "offset": 0})).response;
    assert_eq!(r["error"], "TRANSFER_INVALID");
    // A lock drops an undelivered bundle.
    deliver_bundle(&mut core.lock().unwrap(), big);
    core.lock().unwrap().lock(LockReason::Explicit);
    assert!(core.lock().unwrap().provider.bundle.is_none());
    std::fs::remove_dir_all(&dir).ok();
}
