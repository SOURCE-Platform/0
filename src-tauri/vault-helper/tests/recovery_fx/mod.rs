//! Shared world for the Phase D FsBackupStore rehearsals (§16.7): a
//! synthetic vault on "device A" (Mac) with history, a simulated iPhone
//! enrolled by A, recovery locators + credentials registered, and one
//! publication. Two simulated devices, synthetic credentials, no
//! keychain, nothing printed.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;
use vault_helper::backup::fs_store::{Auth, FsBackupStore, RecoveryKind};
use vault_helper::backup::manifest::SignedManifest;
use vault_helper::backup::snapshot;
use vault_helper::crypto::kdf::{self, Argon2Params};
use vault_helper::crypto::registry::RegistryEntry;
use vault_helper::crypto::secret::{random_secret, SecretBytes};
use vault_helper::recovery::creds;
use vault_helper::registry::build;
use vault_helper::registry::chain::{verify_chain, EpochContext};
use vault_helper::registry::device::{DeviceIdentity, SoftwareDevice, PLATFORM_IOS, PLATFORM_MACOS};
use vault_helper::storage::VaultStore;
use vault_helper::vault::create::create_vault;

pub const EMAIL: &str = "synthetic-owner@example.test";
pub const MP: &[u8] = b"synthetic-recovery-master-password-0001";
pub const MP_NEW: &[u8] = b"synthetic-recovery-master-password-0002";

pub fn tmp(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = PathBuf::from(format!("/tmp/vhrec-{}-{tag}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    d
}

pub struct World {
    pub backup: FsBackupStore,
    pub roots: Vec<PathBuf>,
    pub a_dir: PathBuf,
    pub mac: SoftwareDevice,
    pub phone: SoftwareDevice,
    pub mac_cred: [u8; 32],
    pub vk: SecretBytes<32>,
    pub rk: SecretBytes<32>,
    pub vault_id: [u8; 16],
    pub registry: Vec<RegistryEntry>,
    pub manifest: SignedManifest,
    pub refs: Vec<String>,
}

pub fn world() -> World {
    let backup_root = tmp("store");
    let a_dir = tmp("mac");
    std::fs::create_dir_all(&a_dir).unwrap();
    let backup = FsBackupStore::new(&backup_root);
    let rk = random_secret();
    let (header, vk) = create_vault(&a_dir, MP, &rk).unwrap();
    let mut store = VaultStore::open(&a_dir).unwrap();
    let mut refs = Vec::new();
    for i in 0..3 {
        let meta = format!(r#"{{"title":"Synthetic {i}","username":"u{i}@example.test","hosts":["h{i}.example.test"]}}"#);
        refs.push(store.add_record(&vk, 1, format!(r#"{{"password":"synthetic-pw-{i}"}}"#).as_bytes(), meta.as_bytes()).unwrap());
    }
    store.write_successor(&vk, &refs[0], 1, 1, br#"{"password":"synthetic-pw-0-v2"}"#, br#"{"title":"Synthetic 0 (edited)"}"#, 0).unwrap();
    store.tombstone(&vk, &refs[2]).unwrap();

    let mac = SoftwareDevice::generate("Synthetic Mac", PLATFORM_MACOS);
    let phone = SoftwareDevice::generate("Synthetic iPhone", PLATFORM_IOS);
    let g = build::genesis(&mac).unwrap();
    let st = verify_chain(&[g.clone()], &header.vault_id.0, &NoEpochs).unwrap();
    let registry = vec![g, build::enroll(&st, &mac, &phone).unwrap()];

    let mut mac_cred = [0u8; 32];
    mac_cred.copy_from_slice(random_secret().expose());
    let salt = header.kdf.salt_bytes().unwrap();
    backup
        .create_account(header.vault_id.0, EMAIL, salt, (header.locator_salt_mp.0, header.locator_salt_rk.0), mac.device_id(), &mac_cred)
        .unwrap();
    let auth = Auth::Device { device_id: mac.device_id(), cred: &mac_cred };
    let pk = kdf::derive_pk(MP, &salt, Argon2Params::V1).unwrap();
    let c = creds::mp_creds(&pk, &header.locator_salt_mp.0).unwrap();
    backup.register_recovery(&header.vault_id.0, RecoveryKind::Mp, &c.locator, c.cred.expose(), auth).unwrap();
    let c = creds::rk_creds(&rk, &header.locator_salt_rk.0).unwrap();
    backup.register_recovery(&header.vault_id.0, RecoveryKind::Rk, &c.locator, c.cred.expose(), auth).unwrap();
    let manifest = snapshot::publish(&backup, &store, &registry, None, &mac, &vk, auth).unwrap();
    World {
        backup,
        roots: vec![backup_root, a_dir.clone()],
        a_dir,
        mac,
        phone,
        mac_cred,
        vk,
        rk,
        vault_id: header.vault_id.0,
        registry,
        manifest,
        refs,
    }
}

/// Registries without recovery epochs need no VK.
pub struct NoEpochs;
impl EpochContext for NoEpochs {
    fn vk_for_manifest(&self, _: &[u8; 32]) -> Option<SecretBytes<32>> {
        None
    }
    fn manifest_acceptable(&self, _: &[u8; 32]) -> bool {
        true
    }
}

/// A surviving device that knows `vk` protects `manifest_hash` and has
/// accepted everything in `superseded` as older than its own state.
pub struct KnownVk {
    pub manifest_hash: [u8; 32],
    pub vk: [u8; 32],
    pub superseded: Vec<[u8; 32]>,
}
impl EpochContext for KnownVk {
    fn vk_for_manifest(&self, h: &[u8; 32]) -> Option<SecretBytes<32>> {
        (h == &self.manifest_hash).then(|| SecretBytes::new(self.vk))
    }
    fn manifest_acceptable(&self, h: &[u8; 32]) -> bool {
        !self.superseded.contains(h)
    }
}

impl World {
    pub fn auth_mac(&self) -> Auth<'_> {
        Auth::Device { device_id: self.mac.device_id(), cred: &self.mac_cred }
    }

    /// Device A edits one record and publishes the generation after
    /// `prev`. Returns the new head manifest.
    pub fn a_publishes_edit(&self, prev: &SignedManifest, text: &str) -> SignedManifest {
        let mut store = VaultStore::open(&self.a_dir).unwrap();
        let pt = format!(r#"{{"password":"synthetic-{text}"}}"#);
        store.write_successor(&self.vk, &self.refs[1], 1, 1, pt.as_bytes(), br#"{"title":"Synthetic 1"}"#, 0).unwrap();
        let auth = Auth::Device { device_id: self.mac.device_id(), cred: &self.mac_cred };
        snapshot::publish(&self.backup, &store, &self.registry, Some(prev), &self.mac, &self.vk, auth).unwrap()
    }

    pub fn cleanup(&self, extra: &[&PathBuf]) {
        for d in self.roots.iter().chain(extra.iter().copied()) {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

/// Sorted (ref, list metadata, tip plaintext) for content equality.
pub fn contents(store: &VaultStore, vk: &SecretBytes<32>) -> Vec<(String, Value, Vec<u8>)> {
    let mut out: Vec<(String, Value, Vec<u8>)> = store
        .list_records(vk)
        .unwrap()
        .into_iter()
        .map(|item| {
            let r = item["ref"].as_str().unwrap().to_string();
            let pt = store.read_tip(vk, &r).unwrap().plaintext.to_vec();
            (r, item, pt)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Can this MP/RK recover the provider's *current* state? Locate, derive
/// the recovery credential, fetch the bundle, unwrap — without registry
/// verification (see `second_recovery_blocked_pending_spec_decision`).
pub fn unwraps_current(w: &World, cred: vault_helper::recovery::total_loss::Credential<'_>) -> Result<SecretBytes<32>, vault_helper::errors::ErrorCode> {
    use vault_helper::crypto::wrap::{self, PasswordWrapFile, RecoveryWrapFile};
    use vault_helper::recovery::total_loss::Credential;
    let loc = w.backup.recover_locate(EMAIL)?;
    let (kind, c, key) = match cred {
        Credential::Mp(mp) => {
            let pk = kdf::derive_pk(mp, &loc.kdf_salt, Argon2Params::V1).unwrap();
            (RecoveryKind::Mp, creds::mp_creds(&pk, &loc.locator_salt_mp)?, pk)
        }
        Credential::Rk(rk) => (RecoveryKind::Rk, creds::rk_creds(rk, &loc.locator_salt_rk)?, SecretBytes::new(*rk.expose())),
    };
    let bundle = w.backup.recover_bundle(&loc.vault_id, &c.locator, kind, c.cred.expose())?;
    let d = snapshot::download(&w.backup, &bundle.manifest, Auth::Recovery { kind, cred: c.cred.expose() })?;
    let vk = match kind {
        RecoveryKind::Mp => {
            let f: PasswordWrapFile = serde_json::from_slice(&d.wrap_mp).unwrap();
            wrap::open_wrap_mp(&f, &key, &loc.vault_id).map_err(|_| vault_helper::errors::ErrorCode::WrongCredential)?.vk
        }
        RecoveryKind::Rk => {
            let f: RecoveryWrapFile = serde_json::from_slice(d.wrap_rk.as_ref().unwrap()).unwrap();
            wrap::open_wrap_rk(&f, &key, &loc.vault_id).map_err(|_| vault_helper::errors::ErrorCode::WrongCredential)?.vk
        }
    };
    Ok(vk)
}
