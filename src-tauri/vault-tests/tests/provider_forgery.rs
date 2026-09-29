//! A provider acting alone cannot make a device adopt a key it chose
//! (review SEC-B3). It appends an unproven `recovery_epoch` installing its
//! own device, signs the next manifest with it, seals its own VK′ to the
//! victim's public agreement key (HPKE base mode authenticates no sender),
//! and computes the checkpoint under VK′ — so the checkpoint "anchors"
//! only itself. The victim must refuse: the epoch's proof does not verify
//! under the VK it holds. Nothing is adopted, re-sealed or recorded.
//! Synthetic data only.

mod mfx;

use mfx::*;
use sha2::{Digest, Sha256};
use vault_helper::backup::checkpoint::RegistryCheckpoint;
use vault_helper::backup::index::{IndexEntry, ObjectIndex, Role};
use vault_helper::backup::manifest::SignedManifest;
use vault_helper::crypto::secret::{random_secret, SecretBytes};
use vault_helper::crypto::wrap::DeviceEnvelopePayload;
use vault_helper::device::envelope;
use vault_helper::errors::ErrorCode;
use vault_helper::registry::chain::{verify_chain_with, EpochPolicy};
use vault_helper::registry::device::{DeviceIdentity, SoftwareDevice, PLATFORM_MACOS};
use vault_helper::sync::{fetch, remote};
use vault_proto::request::Operation;

#[test]
fn an_unproven_recovery_epoch_never_hands_over_the_key() {
    let cloud = Cloud::new("forge-epoch");
    let mut a = Mac::new("forge-epoch-a");
    a.setup(&cloud, "synthetic-forge-epoch@example.test").unwrap();
    let mut b = Mac::new("forge-epoch-b");
    a.enroll(&cloud, &b);
    b.join(&cloud, a.vid());
    a.add("secret-record");
    a.publish(&cloud).unwrap();
    b.sync(&cloud).unwrap();
    let vid = b.vid();
    let state = b.read(&cloud, Operation::StateGet, None).body;
    let real = remote::parse(&state).unwrap();
    let get = |h: [u8; 32]| b.read(&cloud, Operation::BlobGet, Some(h)).body;
    let index = ObjectIndex::decode(&get(real.manifest.object_index_hash)).unwrap();
    let blob = |role: &Role| get(index.find(role).unwrap().blob);

    // The provider's device, installed by an epoch it "proves" with VK′.
    let evil = SoftwareDevice::generate("Provider's device", PLATFORM_MACOS);
    let vk_prime: SecretBytes<32> = random_secret();
    let mut entries = vault_helper::registry::file::decode(&blob(&Role::Registry)).unwrap();
    let st = verify_chain_with(&entries, &vid, &EpochPolicy::CheckpointAnchored).unwrap();
    entries.push(vault_helper::registry::build::recovery_epoch(&st, vid, real.manifest_hash, &vk_prime, &evil).unwrap());
    let forged_state = verify_chain_with(&entries, &vid, &EpochPolicy::CheckpointAnchored).expect("structurally valid");
    let registry = vault_helper::registry::file::encode(&entries).unwrap();

    // VK′ sealed to the victim; a header at the next VK generation.
    let next_gen = real.vk_generation + 1;
    let payload = DeviceEnvelopePayload { vk: SecretBytes::new(*vk_prime.expose()), wrapped_at: now(), vk_generation: next_gen };
    let env = envelope::seal_envelope(&b.dev.agree_pub(), &vid, &b.dev.device_id(), &[0x33; 16], &payload).unwrap();
    let env_bytes = serde_json::to_vec_pretty(&env).unwrap();
    let mut header = vault_helper::storage::header::parse_header(&blob(&Role::Header)).unwrap();
    header.vk_generation = next_gen;
    header.registry_head = vault_helper::storage::header::Hex32(forged_state.head);
    let header_bytes = vault_helper::storage::header::write_header(&header).unwrap();
    let wrap_mp = blob(&Role::WrapMp);
    let forged_index = ObjectIndex {
        generation: real.generation + 1,
        entries: vec![
            IndexEntry::of(Role::Header, &header_bytes),
            IndexEntry::of(Role::Registry, &registry),
            IndexEntry::of(Role::WrapMp, &wrap_mp),
            IndexEntry::of(Role::Env { device_id: b.dev.device_id() }, &env_bytes),
        ],
        ..index.clone()
    };
    let manifest = SignedManifest {
        vault_id: vid,
        generation: real.generation + 1,
        created_at: now(),
        registry_head: forged_state.head,
        vk_generation: next_gen,
        object_index_hash: forged_index.hash(),
        prev_manifest_hash: real.manifest_hash,
        signer_device_id: [0; 16],
        signature: [0; 64],
    }
    .sign(&evil)
    .unwrap();
    let checkpoint = RegistryCheckpoint::create(&vk_prime, &manifest, forged_state.epoch).unwrap();
    let (m, cp) = (manifest.encode(), checkpoint.encode());
    let mut v: serde_json::Value = serde_json::from_slice(&state).unwrap();
    let digest = vault_proto::state::recovery_auth_digest(&real.recovery_auth).unwrap();
    let commit = vault_proto::state::state_commit(&vid, manifest.generation, &Sha256::digest(&m).into(), &Sha256::digest(&cp).into(), &digest);
    v["manifest"] = serde_json::json!(vault_proto::b64::encode(&m));
    v["checkpoint"] = serde_json::json!(vault_proto::b64::encode(&cp));
    v["generation"] = serde_json::json!(manifest.generation);
    v["vk_generation"] = serde_json::json!(next_gen);
    v["state_commit"] = serde_json::json!(vault_helper::crypto::hex::encode(commit));
    let forged = remote::parse(&serde_json::to_vec(&v).unwrap()).unwrap();

    // The victim: chained, so it passes the offer; then it must refuse.
    assert_eq!(fetch::offer(b.store(), &forged), Ok(fetch::Offer::Index(forged_index.hash())));
    let blobs: std::collections::HashMap<[u8; 32], Vec<u8>> =
        [header_bytes, registry, wrap_mp, env_bytes].into_iter().map(|x| (sha(&x), x)).collect();
    let (store, vk) = (b.store.take().unwrap(), b.vk.take().unwrap());
    let kept = SecretBytes::new(*vk.expose());
    let tag = b.dev.key_tag().to_string();
    let open = move |f: &envelope::DeviceEnvelopeFile| envelope::open_envelope(&tag, &vid, f);
    let r = vault_helper::sync::apply::apply(store, vk, &forged, &forged_index, &blobs, b.dev.device_id(), &open);
    assert_eq!(r.err(), Some(ErrorCode::SignatureInvalid), "an unproven epoch is refused");
    b.store = Some(vault_helper::storage::VaultStore::open(&b.dir).unwrap());
    b.vk = Some(kept);
    assert_eq!(b.store().header.vk_generation, real.vk_generation, "nothing adopted");
    assert_eq!(b.titles(), vec!["secret-record"], "still sealed under the real VK");
    assert!(vault_helper::storage::compromised::load(&b.store().conn).unwrap().is_none());
}

/// CP-08 — the other direction: a genuine total-loss recovery (its epoch proven
/// under the VK of the state it recovered from) still reaches an
/// up-to-date old device as `Revoked` — refused only when unprovable.
#[test]
fn a_proven_recovery_epoch_still_revokes_the_old_device() {
    use vault_helper::recovery::complete::Plan;
    use vault_helper::recovery::total_loss::Credential;
    use vault_helper::sync::apply::Applied;
    let cloud = Cloud::new("proven-epoch");
    let mut a = Mac::new("proven-epoch-a");
    a.setup(&cloud, "synthetic-proven-epoch@example.test").unwrap();
    a.add("kept");
    a.publish(&cloud).unwrap();
    // A is current; a recovery elsewhere binds A's last accepted manifest.
    let out = mfx::recover::run(&cloud, "synthetic-proven-epoch@example.test", Credential::Mp(MP), Plan { new_mp: None, keep_rk: None }, None).unwrap();
    let recovered = mfx::recover::into_mac(out);
    // A is refused at the provider now, so it is handed the state directly.
    let state = recovered.read(&cloud, Operation::StateGet, None).body;
    let remote = remote::parse(&state).unwrap();
    let index_bytes = recovered.read(&cloud, Operation::BlobGet, Some(remote.manifest.object_index_hash)).body;
    let (index, need) = fetch::plan(a.store(), &remote, &index_bytes).unwrap();
    let blobs: std::collections::HashMap<[u8; 32], Vec<u8>> =
        need.into_iter().map(|h| (h, recovered.read(&cloud, Operation::BlobGet, Some(h)).body)).collect();
    let vid = a.vid();
    let (store, vk) = (a.store.take().unwrap(), a.vk.take().unwrap());
    let tag = a.dev.key_tag().to_string();
    let open = move |f: &envelope::DeviceEnvelopeFile| envelope::open_envelope(&tag, &vid, f);
    let r = vault_helper::sync::apply::apply(store, vk, &remote, &index, &blobs, a.dev.device_id(), &open);
    assert!(matches!(r, Ok(Applied::Revoked(_))), "a proven recovery revokes the old device");
}
