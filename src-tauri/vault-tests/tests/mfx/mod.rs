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
        match apply::apply(store, vk, &remote, &index, &blobs, self.dev.device_id(), &open)? {
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

impl Mac {
    /// Simulated Mac-to-Mac enrollment (§18 Phase F clarification: the
    /// protocol under test is multi-writer publish, merge and revocation;
    /// Mac-to-Mac enrollment is unspecified): this Mac appends an enroll
    /// entry for `other`, seals its envelope, commits locally and publishes.
    pub fn enroll(&mut self, cloud: &Cloud, other: &Mac) {
        use vault_helper::crypto::wrap::DeviceEnvelopePayload;
        use vault_helper::registry::build;
        let vid = self.vid();
        let st = self.registry();
        let entry = build::enroll(&st, &self.dev, &other.dev).unwrap();
        let after = log::append(&self.dir, &vid, &st, entry, &EpochPolicy::CheckpointAnchored).unwrap();
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).unwrap();
        let payload = DeviceEnvelopePayload {
            vk: SecretBytes::new(*self.vk.as_ref().unwrap().expose()),
            wrapped_at: now(),
            vk_generation: self.store().header.vk_generation,
        };
        let env = envelope::seal_envelope(&other.dev.agree_pub(), &vid, &other.dev.device_id(), &nonce, &payload).unwrap();
        envelope::write_envelope(&self.dir, &other.dev.device_id(), &env).unwrap();
        self.store.as_mut().unwrap().set_registry_head(after.head).unwrap();
        self.publish(cloud).expect("publish enrollment");
    }

    /// A device with no local vault materializes the committed state from
    /// its own envelope (§4.7 / §2.10 catch-up).
    pub fn join(&mut self, cloud: &Cloud, vid: [u8; 16]) {
        use vault_helper::sync::join;
        use vault_proto::request::ProviderRequest;
        let none = BTreeSet::new();
        let scope = SignScope { put_blobs: &none, staged: None };
        let get = |op: Operation, blob: Option<[u8; 32]>| {
            let req = SignRequest { operation: op, blob, body_sha256: body_hash(b""), expected_state: None };
            let (pr, h): (ProviderRequest, String) = sign::sign(ORIGIN, vid, Key::Device(&self.dev), &req, &scope, 0, now()).unwrap();
            cloud.send(&pr.method, &pr.path, Some(&h), b"")
        };
        let r = get(Operation::StateGet, None);
        assert_eq!(r.status, 200, "{}", err(&r));
        let remote = remote::parse(&r.body).unwrap();
        let index_bytes = get(Operation::BlobGet, Some(remote.manifest.object_index_hash)).body;
        let index = vault_helper::backup::index::ObjectIndex::decode(&index_bytes).unwrap();
        let mut blobs = HashMap::new();
        for h in index.blobs() {
            blobs.insert(h, get(Operation::BlobGet, Some(h)).body);
        }
        let tag = self.dev.key_tag().to_string();
        let open = move |f: &envelope::DeviceEnvelopeFile| envelope::open_envelope(&tag, &vid, f);
        let vault = self.dir.join("vault");
        let (store, vk) = join::join(&vault, &remote, &index, &blobs, self.dev.device_id(), &open).expect("join");
        self.dir = vault;
        self.store = Some(store);
        self.vk = Some(vk);
    }

    pub fn rk_fresh(&self) -> SecretBytes<32> {
        random_secret()
    }
}

pub mod recover {
    use super::*;
    use vault_helper::recovery::complete::{Completed, Plan};
    use vault_helper::recovery::total_loss::{Credential, Preview, Recovery};

    pub struct Outcome {
        pub preview: Preview,
        pub completed: Completed,
        pub dev: SeDevice,
        pub dir: PathBuf,
    }

    /// Total-loss recovery on a fresh "machine": locate → KDF policy →
    /// derive → signed reads → verify → complete → upload → finalize.
    pub fn run(cloud: &Cloud, handle_text: &str, cred: Credential<'_>, plan: Plan<'_>, tamper_locate: Option<&dyn Fn(&mut serde_json::Value)>) -> Result<Outcome, ErrorCode> {
        let mut locate: serde_json::Value = serde_json::from_slice(&cloud.locate(handle_text)).unwrap();
        if let Some(t) = tamper_locate {
            t(&mut locate);
        }
        let mut r = Recovery::begin(ORIGIN, locate.to_string().as_bytes(), cred, 0)?;
        let none = BTreeSet::new();
        let read = |r: &Recovery, op: Operation, blob: Option<[u8; 32]>| {
            let req = SignRequest { operation: op, blob, body_sha256: body_hash(b""), expected_state: None };
            let h = r.sign(&req, &SignScope { put_blobs: &none, staged: None }, now()).unwrap();
            let (m, p) = op.route(&r.locate.vault_id, blob.as_ref()).unwrap();
            cloud.send(m, &p, Some(&h), b"")
        };
        let s = read(&r, Operation::StateGet, None);
        if s.status != 200 {
            return Err(ErrorCode::WrongCredential);
        }
        let remote = remote::parse(&s.body)?;
        let index_bytes = read(&r, Operation::BlobGet, Some(remote.manifest.object_index_hash)).body;
        let index = r.plan(&remote, &index_bytes)?;
        let mut blobs = HashMap::new();
        for h in index.blobs() {
            blobs.insert(h, read(&r, Operation::BlobGet, Some(h)).body);
        }
        let preview = r.verify(remote, &index, &blobs)?;
        let dir = tmp("recovered");
        let dev = SeDevice::create(&dir, "Synthetic Mac (recovered)", PLATFORM_MACOS).unwrap();
        let completed = r.complete(&dir.join("vault"), &dev, plan)?;
        let st = &completed.staging;
        let put: BTreeSet<[u8; 32]> = st.blobs.keys().copied().collect();
        let scope = SignScope { put_blobs: &put, staged: Some((st.body_sha256, st.expected_state)) };
        let send = |req: SignRequest, body: &[u8]| {
            let h = r.sign(&req, &scope, now()).unwrap();
            let (m, p) = req.operation.route(&r.locate.vault_id, req.blob.as_ref()).unwrap();
            cloud.send(m, &p, Some(&h), body)
        };
        for (h, b) in &st.blobs {
            let resp = send(SignRequest { operation: Operation::BlobPut, blob: Some(*h), body_sha256: body_hash(b), expected_state: None }, b);
            assert_eq!(resp.status, 200, "{}", err(&resp));
        }
        let resp = send(SignRequest { operation: Operation::StateCommit, blob: None, body_sha256: st.body_sha256, expected_state: Some(st.expected_state) }, &st.body);
        if resp.status != 200 {
            return Err(match err(&resp).as_str() {
                "STATE_MOVED" => ErrorCode::StateMoved,
                "FINALIZE_CONFLICT" => ErrorCode::FinalizeConflict,
                _ => ErrorCode::BackupUnavailable,
            });
        }
        let v: serde_json::Value = serde_json::from_slice(&resp.body).unwrap();
        let commit = vault_helper::crypto::hex::decode_array(v["state_commit"].as_str().unwrap()).unwrap();
        publish::committed(&completed.store, st, v["generation"].as_u64().unwrap(), commit)?;
        Ok(Outcome { preview, completed, dev, dir })
    }

    /// Cleanup for an outcome that is not turned into a Mac.
    pub fn discard(o: Outcome) {
        se::delete_keys(o.dev.key_tag());
        let _ = std::fs::remove_dir_all(&o.dir);
    }

    /// The recovered vault as a Mac of the simulator (which then owns
    /// the directory and the SE keys).
    pub fn into_mac(o: Outcome) -> Mac {
        let root = o.dir.clone();
        let dir = root.join("vault");
        let vk = SecretBytes::new(*o.completed.vk.expose());
        let dev = SeDevice::load(&root).unwrap();
        let Outcome { completed, dev: _moved, .. } = o;
        drop(completed);
        let store = vault_helper::storage::VaultStore::open(&dir).unwrap();
        Mac { root, dir, dev, store: Some(store), vk: Some(vk), rk: None }
    }
}
