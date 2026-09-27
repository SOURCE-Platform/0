//! What-if scenarios for sync (spec v0.4 §3.2, §11.5, §16.6, §16.9):
//! SY-11 a revoked author's unseen edit is refused and counted once;
//! BK-04 an older state is a rollback; BK-05 two different states for
//! one generation are fork evidence; BK-02 a tampered blob is detected;
//! BK-11 a manifest signed by a non-enrolled key is rejected. The
//! provider here is honest where the scenario needs it and forged where
//! it plays the attacker. Synthetic data only.

mod mfx;

use mfx::*;
use vault_helper::errors::ErrorCode;
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::storage::rev_state::refused_totals;
use vault_helper::storage::revisions::REFUSED_REVOKED_AUTHOR;
use vault_helper::sync::{fetch, remote};
use vault_proto::request::Operation;

fn pair(tag: &str) -> (Cloud, Mac, Mac) {
    let cloud = Cloud::new(tag);
    let mut a = Mac::new(&format!("{tag}-a"));
    a.setup(&cloud, &format!("synthetic-{tag}@example.test")).unwrap();
    let mut b = Mac::new(&format!("{tag}-b"));
    a.enroll(&cloud, &b);
    b.join(&cloud, a.vid());
    (cloud, a, b)
}

/// SY-11: B publishes an edit A never saw; A revokes B (Admit(B) = what A
/// held); on merge that edit is refused — never shown — and counted once.
/// A's revocation still publishes and cuts B off.
#[test]
fn sy11_unseen_edit_from_revoked_author_is_refused() {
    let (cloud, mut a, mut b) = pair("sy11");
    b.add("sneaked-in");
    b.publish(&cloud).unwrap();
    let (store, vk) = (a.store.take().unwrap(), a.vk.take().unwrap());
    let done = vault_helper::vault::revoke_core::revoke(store, &vk, &a.dev, b.dev.device_id(), MP, &a.rk_fresh()).unwrap();
    a.store = Some(done.store);
    a.vk = Some(done.vk);
    assert_eq!(a.publish(&cloud), Err(ErrorCode::StateMoved));
    a.sync(&cloud).unwrap();
    a.sync(&cloud).unwrap(); // seen again: still counted once
    assert!(!a.titles().contains(&"sneaked-in".to_string()));
    let refused: u64 = refused_totals(&a.store().conn).unwrap().iter().filter(|(r, _)| *r == REFUSED_REVOKED_AUTHOR).map(|(_, n)| n).sum();
    assert_eq!(refused, 1);
    a.publish(&cloud).unwrap();
    assert_eq!(b.read(&cloud, Operation::StateGet, None).status, 401);
}

/// BK-04: a validly signed but older state is refused (rollback floor).
#[test]
fn bk04_older_state_is_a_rollback() {
    let cloud = Cloud::new("bk04");
    let mut a = Mac::new("bk04-a");
    a.setup(&cloud, "synthetic-bk04@example.test").unwrap();
    a.add("one");
    a.publish(&cloud).unwrap();
    let old = a.read(&cloud, Operation::StateGet, None).body;
    a.add("two");
    a.publish(&cloud).unwrap();
    let stale = remote::parse(&old).unwrap();
    assert_eq!(fetch::offer(a.store(), &stale), Err(ErrorCode::ManifestRollback));
}

/// BK-05: two different validly signed manifests for one generation (a
/// provider forking history) are fork evidence, never merged silently.
#[test]
fn bk05_same_generation_different_state_is_a_fork() {
    let cloud = Cloud::new("bk05");
    let mut a = Mac::new("bk05-a");
    a.setup(&cloud, "synthetic-bk05@example.test").unwrap();
    // A second provider copy taken before the next publish.
    let fork_dir = tmp("bk05-fork");
    copy_tree(&cloud.dir, &fork_dir);
    let seen_before = vault_helper::sync::seen::load(&a.store().conn).unwrap().unwrap();
    a.add("history-one");
    a.publish(&cloud).unwrap();
    // The same device state, but a different edit, published to the copy.
    let fork = Cloud::at(&fork_dir);
    a.add("history-two");
    let st = vault_helper::sync::publish::stage_publish(a.store(), &a.registry(), a.vk.as_ref().unwrap(), &a.dev, &seen_before, Vec::new()).unwrap();
    assert_eq!(a.post(&fork, &st).status, 200);
    let forked = remote::parse(&a.read(&fork, Operation::StateGet, None).body).unwrap();
    assert_eq!(fetch::offer(a.store(), &forked), Err(ErrorCode::RegistryFork));
}

/// BK-02: a tampered blob fails its hash — detected, nothing applied.
#[test]
fn bk02_tampered_blob_is_detected() {
    let (cloud, mut a, mut b) = pair("bk02");
    a.add("to-be-tampered");
    a.publish(&cloud).unwrap();
    // Flip one byte in every record blob of the provider copy.
    let vid = vault_helper::crypto::hex::encode(a.vid());
    let blobs = cloud.dir.join(format!("v2/vaults/{vid}/blobs"));
    for e in std::fs::read_dir(&blobs).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none() {
            let mut bytes = std::fs::read(&p).unwrap();
            if bytes.starts_with(b"OV0OBJ02") {
                let n = bytes.len() - 1;
                bytes[n] ^= 1;
                std::fs::write(&p, bytes).unwrap();
            }
        }
    }
    assert_eq!(b.sync(&cloud).err(), Some(ErrorCode::BackupObjectMissing));
    assert!(!b.titles().contains(&"to-be-tampered".to_string()));
}

/// BK-11: a provider-forged state whose manifest is signed by a key the
/// registry never enrolled is rejected by the device.
#[test]
fn bk11_manifest_from_non_enrolled_key_is_rejected() {
    let (cloud, mut a, mut b) = pair("bk11");
    a.add("genuine");
    a.publish(&cloud).unwrap();
    let body = a.read(&cloud, Operation::StateGet, None).body;
    let forged = forge_signer(&body);
    let remote = remote::parse(&forged).unwrap();
    // The device verifies the signer against its registry before trusting anything.
    let idx = remote.manifest.object_index_hash;
    let index_bytes = b.read(&cloud, Operation::BlobGet, Some(idx)).body;
    let (index, need) = fetch::plan(b.store(), &remote, &index_bytes).unwrap();
    let mut blobs = std::collections::HashMap::new();
    for h in need {
        blobs.insert(h, b.read(&cloud, Operation::BlobGet, Some(h)).body);
    }
    let (store, vk) = (b.store.take().unwrap(), b.vk.take().unwrap());
    let tag = b.dev.key_tag().to_string();
    let vid = store.header.vault_id.0;
    let open = move |f: &vault_helper::device::envelope::DeviceEnvelopeFile| vault_helper::device::envelope::open_envelope(&tag, &vid, f);
    let r = vault_helper::sync::apply::apply(store, vk, &remote, &index, &blobs, b.dev.device_id(), &open);
    assert!(r.is_err(), "a foreign signer must never be accepted");
}

/// Re-sign the served manifest with a foreign software key (same fields,
/// foreign signer id) and recompute the state commitment, as a malicious
/// provider could.
fn forge_signer(state_json: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    use vault_helper::backup::manifest::SignedManifest;
    use vault_helper::registry::device::{SoftwareDevice, PLATFORM_MACOS};
    let mut v: serde_json::Value = serde_json::from_slice(state_json).unwrap();
    let m = SignedManifest::decode(&vault_proto::b64::decode(v["manifest"].as_str().unwrap()).unwrap()).unwrap();
    let evil = SoftwareDevice::generate("Forged", PLATFORM_MACOS);
    let forged = m.clone().sign(&evil).unwrap().encode();
    let cp = vault_proto::b64::decode(v["checkpoint"].as_str().unwrap()).unwrap();
    let r = remote::parse(state_json).unwrap();
    let digest = vault_proto::state::recovery_auth_digest(&r.recovery_auth).unwrap();
    let commit = vault_proto::state::state_commit(&m.vault_id, m.generation, &Sha256::digest(&forged).into(), &Sha256::digest(&cp).into(), &digest);
    v["manifest"] = serde_json::json!(vault_proto::b64::encode(&forged));
    v["state_commit"] = serde_json::json!(vault_helper::crypto::hex::encode(commit));
    serde_json::to_vec(&v).unwrap()
}
