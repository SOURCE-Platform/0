//! What-if scenarios for sync (spec v0.4 §3.2, §11.5, §16.6, §16.9):
//! SY-11 a revoked author's unseen edit is refused and counted once;
//! BK-04 an older state is a rollback; BK-05 two different states for
//! one generation are fork evidence; BK-02 a tampered blob is detected;
//! BK-11 a manifest signed by a non-enrolled key is rejected. The
//! provider here is honest where the scenario needs it and forged where
//! it plays the attacker. Fork evidence reaches COMPROMISED only when it
//! verifies (SPEC-B2); the next generation must chain to the accepted one
//! (SPEC-B3, `prev_manifest_hash`). Synthetic data only.

#[path = "../../vault-helper/tests/vault_fx/mod.rs"]
mod vault_fx;
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

/// A provider copy forked from `a`'s state before its next publish, with
/// a different publish at the same generation. Returns (fork, fork dir).
fn forked(cloud: &Cloud, a: &mut Mac, tag: &str) -> Cloud {
    let fork_dir = tmp(tag);
    copy_tree(&cloud.dir, &fork_dir);
    let seen_before = vault_helper::sync::seen::load(&a.store().conn).unwrap().unwrap();
    a.add("history-one");
    a.publish(cloud).unwrap();
    let fork = Cloud::at(&fork_dir);
    a.add("history-two");
    let st = vault_helper::sync::publish::stage_publish(a.store(), &a.registry(), a.vk.as_ref().unwrap(), &a.dev, &seen_before, Vec::new()).unwrap();
    assert_eq!(a.post(&fork, &st).status, 200);
    fork
}

/// SPEC-B3: the next generation not chained to the accepted one is fork
/// evidence (verified: signed by an enrolled device).
#[test]
fn next_generation_off_our_chain_is_a_fork() {
    use vault_helper::sync::seen::Seen;
    let cloud = Cloud::new("prevhash");
    let mut a = Mac::new("prevhash-a");
    a.setup(&cloud, "synthetic-prevhash@example.test").unwrap();
    let fork = forked(&cloud, &mut a, "prevhash-fork");
    let f = remote::parse(&a.read(&fork, Operation::StateGet, None).body).unwrap();
    let on_fork = Seen::with_auth(f.generation, f.manifest_hash, f.state_commit, &f.recovery_auth);
    let st = vault_helper::sync::publish::stage_publish(a.store(), &a.registry(), a.vk.as_ref().unwrap(), &a.dev, &on_fork, Vec::new()).unwrap();
    assert_eq!(a.post(&fork, &st).status, 200);
    let next = remote::parse(&a.read(&fork, Operation::StateGet, None).body).unwrap();
    assert_eq!(next.generation, vault_helper::sync::seen::load(&a.store().conn).unwrap().unwrap().generation + 1);
    assert_eq!(fetch::offer(a.store(), &next), Err(ErrorCode::RegistryFork));
}

/// SPEC-B2 / VER-I2 through the helper op: junk the provider made up is
/// refused with no state change; verified fork evidence enters
/// COMPROMISED, is recorded, drops staged writes, and leaves reads.
#[test]
fn fork_evidence_through_the_op_must_verify() {
    use serde_json::json;
    use vault_helper::state::VaultState;
    let _g = vault_fx::serial();
    let cloud = Cloud::new("forkop");
    let mut a = Mac::new("forkop-a");
    a.setup(&cloud, "synthetic-forkop@example.test").unwrap();
    let fork = forked(&cloud, &mut a, "forkop-fork");
    let mut fx = vault_fx::fx();
    let mut core = vault_helper::vault::VaultCore::boot(a.dir.clone());
    core.store = Some(vault_helper::storage::VaultStore::open(&a.dir).unwrap());
    core.vk = Some(vault_helper::crypto::secret::SecretBytes::new(*a.vk.as_ref().unwrap().expose()));
    core.state = VaultState::Unlocked;
    fx.core = std::sync::Arc::new(std::sync::Mutex::new(core));
    let forked_body = a.read(&fork, Operation::StateGet, None).body;
    // Junk: the same generation re-signed by a key the vault never enrolled.
    let junk = forge_signer(&forked_body);
    let r = fx.op(json!({ "op": "backup_state_offer", "state": String::from_utf8(junk).unwrap() }));
    assert_eq!(r["error"], "SIGNATURE_INVALID", "{r}");
    assert_eq!(fx.state(), VaultState::Unlocked, "unverifiable junk changes nothing");
    assert!(vault_helper::storage::compromised::load(&a.store().conn).unwrap().is_none());
    // Verified evidence: our own key signed both.
    let r = fx.op(json!({ "op": "backup_state_offer", "state": String::from_utf8(forked_body).unwrap() }));
    assert_eq!(r["error"], "REGISTRY_FORK", "{r}");
    assert_eq!(fx.state(), VaultState::Compromised);
    assert!(vault_helper::storage::compromised::load(&a.store().conn).unwrap().is_some(), "evidence recorded");
    assert_eq!(fx.op(json!({ "op": "list_items" }))["ok"], true, "reads allowed");
    assert_eq!(fx.op(json!({ "op": "backup_prepare" }))["error"], "BAD_STATE", "writes frozen");
    let read = fx.op(json!({ "op": "sign_provider_request", "operation": "state_get", "body_sha256": vault_helper::crypto::hex::encode(vault_proto::request::body_hash(b"")) }));
    assert_eq!(read["ok"], true, "network reads allowed: {read}");
    fx.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    fx.core = std::sync::Arc::new(std::sync::Mutex::new(vault_helper::vault::VaultCore::boot(a.dir.clone())));
    assert_eq!(fx.state(), VaultState::Locked);
    fx.remove_dir();
}


/// SPEC-B2 (apply path): the next generation, correctly chained, but
/// with a provider-built registry (its own genesis) and a manifest its own
/// key signed. The served chain verifies on its own terms, yet no device
/// of ours signed it: refused as unverifiable — never COMPROMISED.
#[test]
fn a_provider_built_registry_is_refused_not_a_fork() {
    use sha2::{Digest, Sha256};
    use vault_helper::backup::index::{IndexEntry, ObjectIndex, Role};
    use vault_helper::backup::manifest::SignedManifest;
    use vault_helper::registry::device::{SoftwareDevice, PLATFORM_MACOS};
    let (cloud, mut a, b) = pair("junkreg");
    a.add("genuine");
    a.publish(&cloud).unwrap();
    let state = b.read(&cloud, Operation::StateGet, None).body;
    let real = remote::parse(&state).unwrap();
    let get = |h: [u8; 32]| b.read(&cloud, Operation::BlobGet, Some(h)).body;
    let index = ObjectIndex::decode(&get(real.manifest.object_index_hash)).unwrap();
    let evil = SoftwareDevice::generate("Provider's device", PLATFORM_MACOS);
    // Longer than ours, so it is a divergence, not a truncation.
    let vid = b.vid();
    let mut chain_entries = vec![vault_helper::registry::build::genesis(&evil).unwrap()];
    for i in 0..3 {
        let st = vault_helper::registry::chain::verify_chain_with(&chain_entries, &vid, &vault_helper::registry::chain::EpochPolicy::CheckpointAnchored).unwrap();
        let other = SoftwareDevice::generate(&format!("Provider's device {i}"), PLATFORM_MACOS);
        chain_entries.push(vault_helper::registry::build::enroll(&st, &evil, &other).unwrap());
    }
    let fake_reg = vault_helper::registry::file::encode(&chain_entries).unwrap();
    let mut entries: Vec<IndexEntry> = index.entries.iter().filter(|e| e.role != Role::Registry).cloned().collect();
    entries.push(IndexEntry::of(Role::Registry, &fake_reg));
    let fake_index = ObjectIndex { entries, ..index.clone() };
    let mut m: SignedManifest = real.manifest.clone();
    m.object_index_hash = fake_index.hash();
    m.registry_head = vault_helper::crypto::registry::entry_hash(chain_entries.last().unwrap()).unwrap();
    m.signer_device_id = evil.device_id();
    let m = m.sign(&evil).unwrap().encode();
    let mut v: serde_json::Value = serde_json::from_slice(&state).unwrap();
    let digest = vault_proto::state::recovery_auth_digest(&real.recovery_auth).unwrap();
    let commit = vault_proto::state::state_commit(&real.manifest.vault_id, real.generation, &Sha256::digest(&m).into(), &Sha256::digest(&real.checkpoint_bytes).into(), &digest);
    v["manifest"] = serde_json::json!(vault_proto::b64::encode(&m));
    v["state_commit"] = serde_json::json!(vault_helper::crypto::hex::encode(commit));
    let forged = remote::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(fetch::offer(b.store(), &forged), Ok(fetch::Offer::Index(fake_index.hash())), "chained: passes the offer");
    let (plan_index, need) = fetch::plan(b.store(), &forged, &fake_index.encode()).unwrap();
    let mut blobs = std::collections::HashMap::new();
    for h in need {
        let bytes = if h == sha(&fake_reg) { fake_reg.clone() } else { get(h) };
        blobs.insert(h, bytes);
    }
    let dir = b.dir.clone();
    let mut b = b;
    let (store, vk) = (b.store.take().unwrap(), b.vk.take().unwrap());
    let tag = b.dev.key_tag().to_string();
    let open = move |f: &vault_helper::device::envelope::DeviceEnvelopeFile| vault_helper::device::envelope::open_envelope(&tag, &vid, f);
    let r = vault_helper::sync::apply::apply(store, vk, &forged, &plan_index, &blobs, b.dev.device_id(), &open);
    assert_eq!(r.err(), Some(ErrorCode::SignatureInvalid));
    let reopened = vault_helper::storage::VaultStore::open(&dir).unwrap();
    assert!(vault_helper::storage::compromised::load(&reopened.conn).unwrap().is_none(), "no evidence recorded");
}
