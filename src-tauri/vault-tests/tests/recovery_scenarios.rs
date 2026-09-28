//! What-if scenarios on the recovery side (spec v0.4 §12, §4.8, §16.6,
//! §16.7): RC-05 MP forgotten with a trusted device, RC-07 a suspected
//! stolen RK (security-driven, old RK authenticates until the commit),
//! BK-10 a historical snapshot and the old RK, CP-02 a substituted
//! registry, CP-03 an altered historical recovery_epoch, BK-18 no
//! canary secret anywhere in provider storage, CP-01
//! repeated recoveries, RU-03 a pending change across a restart.
//! Synthetic data only.

mod mfx;

use std::collections::{BTreeSet, HashMap};

use mfx::recover::{self, into_mac};
use mfx::*;
use vault_helper::crypto::secret::SecretBytes;
use vault_helper::recovery::complete::Plan;
use vault_helper::recovery::total_loss::{Credential, Recovery};
use vault_helper::sync::sign::{SignRequest, SignScope};
use vault_helper::sync::{pending, remote};
use vault_helper::vault::recovery_ops::{change_mp, prove_mp, rotate_recovery_key};
use vault_proto::request::{body_hash, Operation};

const NEW_MP: &[u8] = b"synthetic-e2e-master-password-reset-01";

fn world(tag: &str) -> (Cloud, Mac, String) {
    let cloud = Cloud::new(tag);
    let handle = format!("synthetic-{tag}@example.test");
    let mut mac = Mac::new(&format!("{tag}-mac"));
    mac.setup(&cloud, &handle).unwrap();
    mac.add("kept");
    mac.publish(&cloud).unwrap();
    (cloud, mac, handle)
}

/// Can this credential authenticate a recovery-class read right now?
fn can_read(cloud: &Cloud, handle: &str, cred: Credential<'_>) -> u16 {
    let locate = cloud.locate(handle);
    let r = Recovery::begin(ORIGIN, &locate, cred, 0).unwrap();
    let none = BTreeSet::new();
    let req = SignRequest { operation: Operation::StateGet, blob: None, body_sha256: body_hash(b""), expected_state: None };
    let h = r.sign(&req, &SignScope { put_blobs: &none, staged: None }, now()).unwrap();
    let (m, p) = Operation::StateGet.route(&r.locate.vault_id, None).unwrap();
    cloud.send(m, &p, Some(&h), b"").status
}

/// RC-05: the MP is forgotten on a trusted Mac: a new one is set; after
/// the publish the provider accepts only the new MP for recovery.
#[test]
fn rc05_mp_reset_with_a_trusted_device() {
    let (cloud, mut mac, handle) = world("rc05");
    let (store, vk) = (mac.store.take().unwrap(), mac.vk.take().unwrap());
    mac.store = Some(change_mp(store, &vk, None, NEW_MP).unwrap());
    mac.vk = Some(vk);
    assert_eq!(can_read(&cloud, &handle, Credential::Mp(MP)), 200, "old MP works until the publish (REMOTE_UPDATE_PENDING)");
    mac.publish(&cloud).unwrap();
    assert_eq!(can_read(&cloud, &handle, Credential::Mp(NEW_MP)), 200);
    assert_ne!(can_read(&cloud, &handle, Credential::Mp(MP)), 200);
    let out = recover::run(&cloud, &handle, Credential::Mp(NEW_MP), Plan { new_mp: None, keep_rk: None }, None).unwrap();
    assert_eq!(into_mac(out).titles(), vec!["kept"]);
}

/// RC-07: a suspected-stolen RK is replaced as security-driven work; the
/// old RK authenticates until the transition commits, then never again.
#[test]
fn rc07_suspected_stolen_rk() {
    let (cloud, mut mac, handle) = world("rc07");
    let old_rk = SecretBytes::new(*mac.rk.as_ref().unwrap().expose());
    let (store, vk) = (mac.store.take().unwrap(), mac.vk.take().unwrap());
    let pk = prove_mp(&store, MP).unwrap();
    let rot = rotate_recovery_key(store, &vk, &pk, true).unwrap();
    mac.store = Some(vault_helper::storage::VaultStore::open(&mac.dir).unwrap());
    mac.vk = Some(rot.rotation.new_vk);
    assert!(pending::load(&mac.store().conn).unwrap().unwrap().security_driven);
    assert_eq!(can_read(&cloud, &handle, Credential::Rk(&old_rk)), 200, "not yet cut off");
    mac.publish(&cloud).unwrap();
    assert_ne!(can_read(&cloud, &handle, Credential::Rk(&old_rk)), 200, "cut off at the commit");
    assert_eq!(can_read(&cloud, &handle, Credential::Rk(&rot.new_rk)), 200);
}

/// BK-10: the retained previous generation's recovery.wrap still opens
/// with the old RK (the §12 scenario 7 limitation, as tested behaviour);
/// the current state's wrap refuses it.
#[test]
fn bk10_historical_snapshot_and_the_old_rk() {
    let (cloud, mut mac, _handle) = world("bk10");
    let old_rk = SecretBytes::new(*mac.rk.as_ref().unwrap().expose());
    let old_wrap = std::fs::read(mac.dir.join(vault_helper::storage::store::RECOVERY_WRAP_NAME)).unwrap();
    let (store, vk) = (mac.store.take().unwrap(), mac.vk.take().unwrap());
    let pk = prove_mp(&store, MP).unwrap();
    let rot = rotate_recovery_key(store, &vk, &pk, false).unwrap();
    mac.store = Some(vault_helper::storage::VaultStore::open(&mac.dir).unwrap());
    mac.vk = Some(rot.rotation.new_vk);
    mac.publish(&cloud).unwrap();
    let vid = mac.vid();
    let open = |bytes: &[u8]| {
        let f: vault_helper::crypto::wrap::RecoveryWrapFile = serde_json::from_slice(bytes).unwrap();
        vault_helper::crypto::wrap::open_wrap_rk(&f, &old_rk, &vid).map(|p| p.vk_generation)
    };
    // The old wrap is still a blob in the provider (retained generation).
    let blob = cloud.dir.join(format!("v2/vaults/{}/blobs/{}", vault_helper::crypto::hex::encode(vid), vault_helper::crypto::hex::encode(sha(&old_wrap))));
    assert_eq!(open(&std::fs::read(blob).unwrap()), Ok(1), "historical snapshot opens with the old RK");
    let current = std::fs::read(mac.dir.join(vault_helper::storage::store::RECOVERY_WRAP_NAME)).unwrap();
    assert!(open(&current).is_err(), "the current state refuses the old RK");
}

/// CP-02: a provider that substitutes a registry (its own manifest over a
/// new index) cannot make the served checkpoint bind it: recovery stops.
#[test]
fn cp02_substituted_registry_is_refused() {
    let (cloud, mac, handle) = world("cp02");
    let evil = vault_helper::registry::device::SoftwareDevice::generate("Provider's device", vault_helper::registry::device::PLATFORM_MACOS);
    let fake = vault_helper::registry::file::encode(&[vault_helper::registry::build::genesis(&evil).unwrap()]).unwrap();
    let rk = SecretBytes::new(*mac.rk.as_ref().unwrap().expose());
    assert_eq!(substituted_registry_error(&cloud, &mac, &handle, &rk, &evil, fake), vault_helper::errors::ErrorCode::ManifestMismatch, "the checkpoint step refuses it");
}

/// CP-03: after a recovery (a `recovery_epoch` entry in the registry), a
/// provider that alters that historical entry is refused both ways: the
/// altered blob fails its index hash, and a rebuilt index/manifest fails
/// the checkpoint binding.
#[test]
fn cp03_altered_recovery_epoch_is_refused() {
    let (cloud, mac, handle) = world("cp03");
    let rk = SecretBytes::new(*mac.rk.as_ref().unwrap().expose());
    drop(mac);
    let out = recover::run(&cloud, &handle, Credential::Mp(MP), Plan { new_mp: None, keep_rk: Some(&rk) }, None).unwrap();
    let mut m = into_mac(out);
    m.add("after-recovery");
    m.publish(&cloud).unwrap();
    let real = std::fs::read(m.dir.join(vault_helper::VAULT_REGISTRY_NAME)).unwrap();
    // A well-formed registry whose historical recovery_epoch differs.
    let mut entries = vault_helper::registry::file::decode(&real).unwrap();
    let epoch = entries.iter_mut().rev().find(|e| e.kind == vault_helper::crypto::registry::EntryKind::RecoveryEpoch).expect("a recovery_epoch entry");
    match epoch.recovery_proof.as_mut() {
        Some(p) => p[0] ^= 1,
        None => epoch.enrolled_at = epoch.enrolled_at.map(|t| t + 1),
    }
    let altered = vault_helper::registry::file::encode(&entries).unwrap();
    assert_ne!(altered, real);
    // Rebuilt index + manifest over it: the checkpoint binding refuses it.
    let evil = vault_helper::registry::device::SoftwareDevice::generate("Provider's device", vault_helper::registry::device::PLATFORM_MACOS);
    assert_eq!(substituted_registry_error(&cloud, &m, &handle, &rk, &evil, altered.clone()), vault_helper::errors::ErrorCode::ManifestMismatch);
    // In place: the served blob no longer matches its index hash.
    let blob = cloud.dir.join(format!("v2/vaults/{}/blobs/{}", vault_helper::crypto::hex::encode(m.vid()), vault_helper::crypto::hex::encode(sha(&real))));
    std::fs::write(&blob, &altered).unwrap();
    let r = recover::run(&cloud, &handle, Credential::Mp(MP), Plan { new_mp: None, keep_rk: Some(&rk) }, None);
    assert_eq!(r.err(), Some(vault_helper::errors::ErrorCode::BackupObjectMissing));
}

/// Serve `fake_reg` in place of the registry, with a new index and a
/// manifest re-signed by `evil`; the error recovery's verify returns.
fn substituted_registry_error(cloud: &Cloud, mac: &Mac, handle: &str, rk: &SecretBytes<32>, evil: &vault_helper::registry::device::SoftwareDevice, fake_reg: Vec<u8>) -> vault_helper::errors::ErrorCode {
    use vault_helper::backup::index::{IndexEntry, ObjectIndex, Role};
    use vault_helper::backup::manifest::SignedManifest;
    let state = mac.read(cloud, Operation::StateGet, None).body;
    let real = remote::parse(&state).unwrap();
    let get = |h: [u8; 32]| mac.read(cloud, Operation::BlobGet, Some(h)).body;
    let index = ObjectIndex::decode(&get(real.manifest.object_index_hash)).unwrap();
    let mut blobs: HashMap<[u8; 32], Vec<u8>> = index.blobs().into_iter().map(|h| (h, get(h))).collect();
    let mut entries: Vec<IndexEntry> = index.entries.iter().filter(|e| e.role != Role::Registry).cloned().collect();
    entries.push(IndexEntry::of(Role::Registry, &fake_reg));
    let fake_reg_bytes = fake_reg.clone();
    blobs.insert(sha(&fake_reg), fake_reg);
    let fake_index = ObjectIndex { entries, ..index.clone() };
    blobs.insert(fake_index.hash(), fake_index.encode());
    let mut m: SignedManifest = real.manifest.clone();
    m.object_index_hash = fake_index.hash();
    // The manifest names the substituted registry's head, as a real
    // attacker's would: only the checkpoint binding can still refuse it.
    let fake_entries = vault_helper::registry::file::decode(blobs.get(&sha(&fake_reg_bytes)).unwrap()).unwrap();
    m.registry_head = vault_helper::crypto::registry::entry_hash(fake_entries.last().unwrap()).unwrap();
    m.signer_device_id = vault_helper::registry::device::DeviceIdentity::device_id(evil);
    let m = m.sign(evil).unwrap();
    let forged = RemoteStateBuilder::with_manifest(&state, &m.encode());
    let locate = cloud.locate(handle);
    let mut r = Recovery::begin(ORIGIN, &locate, Credential::Rk(rk), 0).unwrap();
    let remote = remote::parse(&forged).unwrap();
    let idx = r.plan(&remote, &fake_index.encode()).expect("the forged index is well formed");
    r.verify(remote, &idx, &blobs).expect_err("a substituted registry never verifies")
}

/// BK-18: canary secrets planted in a full flow never appear anywhere in
/// provider storage (state, blobs, nonces, claims, throttle slots).
#[test]
fn bk18_no_canary_in_provider_storage() {
    let (cloud, mut mac, handle) = world("bk18");
    let canary_pw = "synthetic-canary-password-7f3a9c";
    let meta = r#"{"title":"canary","username":"canary@example.test","hosts":[]}"#;
    let vk = SecretBytes::new(*mac.vk.as_ref().unwrap().expose());
    mac.store.as_mut().unwrap().add_record(&vk, 1, format!(r#"{{"password":"{canary_pw}"}}"#).as_bytes(), meta.as_bytes()).unwrap();
    mac.publish(&cloud).unwrap();
    recover::discard(recover::run(&cloud, &handle, Credential::Mp(MP), Plan { new_mp: None, keep_rk: None }, None).expect("the recovery ran"));
    let h = mac.store().header.clone();
    let pk = vault_helper::crypto::kdf::derive_pk(MP, &h.kdf.salt.0, vault_helper::storage::header::kdf_params(&h.kdf)).unwrap();
    let ikm = |class: vault_helper::crypto::recovery_auth::RecoveryClass, s: &[u8], salt: &[u8; 16]| {
        let mut info = class.domain().to_vec();
        info.extend_from_slice(&h.vault_id.0);
        vault_proto::crypto::hkdf::hkdf32(s, salt, &info).unwrap()
    };
    let ikm_mp = ikm(vault_helper::crypto::recovery_auth::RecoveryClass::Mp, pk.expose(), &h.auth_salt_mp.0);
    let ikm_rk = ikm(vault_helper::crypto::recovery_auth::RecoveryClass::Rk, mac.rk.as_ref().unwrap().expose(), &h.auth_salt_rk.0);
    let mut canaries: Vec<Vec<u8>> = vec![
        pk.expose().to_vec(),
        ikm_mp.expose().to_vec(),
        ikm_rk.expose().to_vec(),
        MP.to_vec(),
        canary_pw.as_bytes().to_vec(),
        vk.expose().to_vec(),
        mac.rk.as_ref().unwrap().expose().to_vec(),
        vault_helper::crypto::hex::encode(vk.expose()).into_bytes(),
    ];
    for i in [&ikm_mp, &ikm_rk] {
        for counter in [0u8, 1] {
            canaries.push(vault_proto::crypto::recovery_auth::dkp_candidate(i.expose(), counter).to_vec()); // sk_c
        }
    }
    let mut files = 0;
    scan(&cloud.dir, &mut |bytes| {
        files += 1;
        for c in &canaries {
            assert!(!bytes.windows(c.len()).any(|w| w == c.as_slice()), "a canary reached provider storage");
        }
    });
    assert!(files > 10, "the scan covered the provider store");
}

fn scan(dir: &std::path::Path, f: &mut dyn FnMut(&[u8])) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            scan(&p, f);
        } else {
            f(&std::fs::read(p).unwrap());
        }
    }
}

/// A served state with its manifest replaced and the commitment
/// recomputed, as a malicious provider would serve it.
struct RemoteStateBuilder;
impl RemoteStateBuilder {
    fn with_manifest(state_json: &[u8], manifest: &[u8]) -> Vec<u8> {
        use sha2::{Digest, Sha256};
        let mut v: serde_json::Value = serde_json::from_slice(state_json).unwrap();
        let r = remote::parse(state_json).unwrap();
        let m = vault_helper::backup::manifest::SignedManifest::decode(manifest).unwrap();
        let digest = vault_proto::state::recovery_auth_digest(&r.recovery_auth).unwrap();
        let commit = vault_proto::state::state_commit(&m.vault_id, m.generation, &Sha256::digest(manifest).into(), &Sha256::digest(&r.checkpoint_bytes).into(), &digest);
        v["manifest"] = serde_json::json!(vault_proto::b64::encode(manifest));
        v["state_commit"] = serde_json::json!(vault_helper::crypto::hex::encode(commit));
        serde_json::to_vec(&v).unwrap()
    }
}

/// CP-01: repeated total-loss recoveries — each one re-keys, moves the
/// registry epoch on and publishes; the next recovery verifies the new
/// checkpoint and gets everything, including edits made in between.
#[test]
fn cp01_repeated_recoveries() {
    let (cloud, mac, handle) = world("cp01");
    let rk = SecretBytes::new(*mac.rk.as_ref().unwrap().expose());
    drop(mac);
    let mut expected = vec!["kept".to_string()];
    for round in 1..=3 {
        let out = recover::run(&cloud, &handle, Credential::Mp(MP), Plan { new_mp: None, keep_rk: Some(&rk) }, None).expect("recovered");
        let mut m = into_mac(out);
        assert_eq!(m.titles(), expected, "round {round}");
        assert_eq!(m.store().header.vk_generation, 1 + round, "one rotation per recovery");
        assert_eq!(m.registry().epoch, round as u64, "one epoch per recovery; earlier epochs stay as history");
        let epochs = m.registry().entries.iter().filter(|e| e.kind == vault_helper::crypto::registry::EntryKind::RecoveryEpoch).count();
        assert_eq!(epochs, round as usize);
        let title = format!("after-round-{round}");
        m.add(&title);
        m.publish(&cloud).unwrap();
        expected.push(title);
        expected.sort();
    }
}

/// RU-03: a wrap-bearing change waiting for its publish survives a restart
/// (the store is reopened) and is cleared by the publish that carries it.
#[test]
fn ru03_pending_change_survives_a_restart() {
    let (cloud, mut mac, handle) = world("ru03");
    let (store, vk) = (mac.store.take().unwrap(), mac.vk.take().unwrap());
    drop(change_mp(store, &vk, None, NEW_MP).unwrap());
    // "Restart": nothing but the vault directory carries over.
    mac.store = Some(vault_helper::storage::VaultStore::open(&mac.dir).unwrap());
    mac.vk = Some(vk);
    let p = pending::load(&mac.store().conn).unwrap().expect("pending survives the reopen");
    assert!(!p.needs_user);
    mac.publish(&cloud).unwrap();
    assert!(pending::load(&mac.store().conn).unwrap().is_none(), "cleared by the publish");
    assert_eq!(can_read(&cloud, &handle, Credential::Mp(NEW_MP)), 200);
}
