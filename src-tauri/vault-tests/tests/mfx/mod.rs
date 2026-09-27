//! Main-process simulator for end-to-end tests (spec v0.4 §11.1): the
//! provider core over `FsStores` in-process, and "Macs" — helper engines
//! over real vault directories with Secure-Enclave test identities in the
//! run's `test.` namespace. The simulator plays main: it asks the helper
//! engine to sign, moves bytes, and reports results. Synthetic data only.
#![allow(dead_code)]

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use vault_helper::crypto::kdf;
use vault_helper::crypto::secret::{random_secret, SecretBytes};
use vault_helper::device::{envelope, se, SeDevice};
use vault_helper::errors::ErrorCode;
use vault_helper::registry::chain::{EpochPolicy, RegistryState};
use vault_helper::registry::device::{DeviceIdentity, PLATFORM_MACOS};
use vault_helper::registry::log;
use vault_helper::storage::header::kdf_params;
use vault_helper::storage::VaultStore;
use vault_helper::sync::apply::{self, Applied, Report};
use vault_helper::sync::fetch::{self, Offer};
use vault_helper::sync::publish::{self, Staging};
use vault_helper::sync::sign::{self, Key, SignRequest, SignScope};
use vault_helper::sync::{remote, seen};
use vault_helper::vault::create::create_vault;
use vault_proto::handle;
use vault_proto::request::{body_hash, Operation};
use vault_provider_core::fs::FsStores;
use vault_provider_core::{Config, Provider, Request, Response};

pub const ORIGIN: &str = "https://provider.test";
pub const MP: &[u8] = b"synthetic-e2e-master-password-0001";

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

pub fn tmp(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("vte-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Recursive copy (provider-store snapshots for fork scenarios).
pub fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dst = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &dst);
        } else {
            std::fs::copy(e.path(), dst).unwrap();
        }
    }
}

pub fn sha(b: &[u8]) -> [u8; 32] {
    Sha256::digest(b).into()
}

pub struct Cloud {
    pub p: Arc<Provider>,
    pub fs: Arc<FsStores>,
    pub dir: PathBuf,
}

impl Drop for Cloud {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Cloud {
    pub fn new(tag: &str) -> Cloud {
        vault_helper::test_support::init_test_namespace();
        let dir = tmp(&format!("{tag}-cloud"));
        let fs = Arc::new(FsStores::new(&dir));
        Cloud { p: Arc::new(Provider::with_fs(Config::new(ORIGIN, [0x5e; 32]), fs.clone())), fs, dir }
    }

    /// A second provider instance over an existing store directory.
    pub fn at(dir: &std::path::Path) -> Cloud {
        let fs = Arc::new(FsStores::new(dir));
        Cloud { p: Arc::new(Provider::with_fs(Config::new(ORIGIN, [0x5e; 32]), fs.clone())), fs, dir: dir.to_path_buf() }
    }

    pub fn send(&self, method: &str, path: &str, auth: Option<&str>, body: &[u8]) -> Response {
        self.p.handle(&Request { method, path, auth, body, now: now(), client_ip: "192.0.2.10" })
    }

    pub fn locate(&self, handle_text: &str) -> Vec<u8> {
        let hk = handle::handle_key(&handle::normalize(handle_text).unwrap());
        let body = serde_json::json!({ "handle_key": vault_helper::crypto::hex::encode(hk) }).to_string();
        let r = self.send("POST", "/v2/recover/locate", None, body.as_bytes());
        assert_eq!(r.status, 200);
        r.body
    }
}

pub struct Mac {
    /// Everything this simulated machine wrote (removed on drop).
    pub root: PathBuf,
    /// The vault directory (the root, or `root/vault` after join/recovery).
    pub dir: PathBuf,
    pub dev: SeDevice,
    pub store: Option<VaultStore>,
    pub vk: Option<SecretBytes<32>>,
    pub rk: Option<SecretBytes<32>>,
}

impl Drop for Mac {
    fn drop(&mut self) {
        se::delete_keys(self.dev.key_tag());
        self.store = None;
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub fn err(r: &Response) -> String {
    serde_json::from_slice::<serde_json::Value>(&r.body).ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_default()
}

impl Mac {
    pub fn new(tag: &str) -> Mac {
        vault_helper::test_support::init_test_namespace();
        let dir = tmp(tag);
        let dev = SeDevice::create(&dir, &format!("Synthetic Mac {tag}"), PLATFORM_MACOS).expect("SE identity");
        Mac { root: dir.clone(), dir, dev, store: None, vk: None, rk: None }
    }

    pub fn store(&self) -> &VaultStore {
        self.store.as_ref().unwrap()
    }

    pub fn registry(&self) -> RegistryState {
        log::read_state(&self.dir, &self.store().header.vault_id.0, &EpochPolicy::CheckpointAnchored).unwrap()
    }

    pub fn vid(&self) -> [u8; 16] {
        self.store().header.vault_id.0
    }

    /// Sign as this device with `scope` and send.
    pub fn call(&self, cloud: &Cloud, req: SignRequest, scope: &SignScope<'_>, body: &[u8]) -> Response {
        let (pr, h) = sign::sign(ORIGIN, self.vid(), Key::Device(&self.dev), &req, scope, 0, now()).expect("sign");
        cloud.send(&pr.method, &pr.path, Some(&h), body)
    }

    pub fn read(&self, cloud: &Cloud, op: Operation, blob: Option<[u8; 32]>) -> Response {
        let none = BTreeSet::new();
        self.call(cloud, SignRequest { operation: op, blob, body_sha256: body_hash(b""), expected_state: None }, &SignScope { put_blobs: &none, staged: None }, b"")
    }

    /// Upload a staging's blobs, then post its body (§11.3 flow).
    pub fn post(&self, cloud: &Cloud, st: &Staging) -> Response {
        let put: BTreeSet<[u8; 32]> = st.blobs.keys().copied().collect();
        let scope = SignScope { put_blobs: &put, staged: Some((st.body_sha256, st.expected_state)) };
        if st.kind != vault_proto::state::TransitionKind::Create {
            for (h, b) in &st.blobs {
                let r = self.call(cloud, SignRequest { operation: Operation::BlobPut, blob: Some(*h), body_sha256: body_hash(b), expected_state: None }, &scope, b);
                assert_eq!(r.status, 200, "blob_put {}", err(&r));
            }
        }
        self.call(cloud, SignRequest { operation: Operation::StateCommit, blob: None, body_sha256: st.body_sha256, expected_state: Some(st.expected_state) }, &scope, &st.body)
    }

    fn accept(&mut self, st: &Staging, r: &Response) -> Result<(), ErrorCode> {
        if r.status != 200 {
            return Err(match err(r).as_str() {
                "STATE_MOVED" => ErrorCode::StateMoved,
                "HANDLE_TAKEN" => ErrorCode::HandleTaken,
                _ => ErrorCode::BackupUnavailable,
            });
        }
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        let commit = vault_helper::crypto::hex::decode_array(v["state_commit"].as_str().unwrap()).unwrap();
        publish::committed(self.store(), st, v["generation"].as_u64().unwrap(), commit).map(|_| ())
    }

    /// Setup: a local vault, then the `create` transition.
    pub fn setup(&mut self, cloud: &Cloud, handle_text: &str) -> Result<(), ErrorCode> {
        let rk = random_secret();
        let (header, vk) = create_vault(&self.dir, MP, &rk, &self.dev).unwrap();
        let pk = kdf::derive_pk(MP, &header.kdf.salt.0, kdf_params(&header.kdf)).unwrap();
        let updates = publish::recovery_updates(&header, Some(&pk), Some(&rk)).unwrap();
        self.store = Some(VaultStore::open(&self.dir).unwrap());
        self.vk = Some(vk);
        self.rk = Some(rk);
        let hk = handle::handle_key(&handle::normalize(handle_text).unwrap());
        let st = publish::stage_create(self.store(), &self.registry(), self.vk.as_ref().unwrap(), &self.dev, hk, updates)?;
        let r = self.post(cloud, &st);
        self.accept(&st, &r)
    }

    pub fn add(&mut self, title: &str) -> String {
        let meta = format!(r#"{{"title":"{title}","username":"u@example.test","hosts":["{title}.example.test"]}}"#);
        let pt = format!(r#"{{"password":"synthetic-{title}"}}"#);
        let vk = self.vk.as_ref().unwrap();
        self.store.as_mut().unwrap().add_record(vk, 1, pt.as_bytes(), meta.as_bytes()).unwrap()
    }

    pub fn publish(&mut self, cloud: &Cloud) -> Result<(), ErrorCode> {
        let seen = seen::load(&self.store().conn).unwrap().expect("seen");
        let st = publish::stage_publish(self.store(), &self.registry(), self.vk.as_ref().unwrap(), &self.dev, &seen, Vec::new())?;
        let r = self.post(cloud, &st);
        self.accept(&st, &r)
    }

    /// Fetch, verify and merge the provider's current state.
    pub fn sync(&mut self, cloud: &Cloud) -> Result<Option<Report>, ErrorCode> {
        let r = self.read(cloud, Operation::StateGet, None);
        if r.status != 200 {
            return Err(ErrorCode::AuthInvalid);
        }
        let remote = remote::parse(&r.body)?;
        let idx = match fetch::offer(self.store(), &remote)? {
            Offer::UpToDate => return Ok(None),
            Offer::Index(h) => h,
        };
        let index_bytes = self.read(cloud, Operation::BlobGet, Some(idx)).body;
        let (index, need) = fetch::plan(self.store(), &remote, &index_bytes)?;
        let mut blobs = HashMap::new();
        for h in need {
            let b = self.read(cloud, Operation::BlobGet, Some(h));
            assert_eq!(b.status, 200);
            blobs.insert(h, b.body);
        }
        let tag = self.dev.key_tag().to_string();
        let vid = self.vid();
        let open = move |f: &envelope::DeviceEnvelopeFile| envelope::open_envelope(&tag, &vid, f);
        let store = self.store.take().unwrap();
        let vk = self.vk.take().unwrap();
        // Like the helper op: on failure reopen the committed vault (the
        // apply is transactional/journaled) and keep the key resident.
        let keep = SecretBytes::new(*vk.expose());
        let applied = apply::apply(store, vk, &remote, &index, &blobs, self.dev.device_id(), &open);
        let applied = match applied {
            Ok(a) => a,
            Err(e) => {
                self.store = VaultStore::open(&self.dir).ok();
                self.vk = Some(keep);
                return Err(e);
            }
        };
        match applied {
            Applied::Merged(rep, store, vk) => {
                self.store = Some(store);
                self.vk = Some(vk);
                Ok(Some(rep))
            }
            Applied::Revoked(_) => Err(ErrorCode::DeviceNotAuthorized),
        }
    }

    pub fn titles(&self) -> Vec<String> {
        let mut t: Vec<String> = self
            .store()
            .list_records(self.vk.as_ref().unwrap())
            .unwrap()
            .iter()
            .filter_map(|i| i["title"].as_str().map(String::from))
            .collect();
        t.sort();
        t
    }
}

mod multi;
pub mod recover;
