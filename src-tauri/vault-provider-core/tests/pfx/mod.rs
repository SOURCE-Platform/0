//! Provider test fixture: a synthetic vault driven through the real
//! `Provider` over `FsStores` with signed requests. Keys are software test
//! identities; PK/RK/VK are random synthetic secrets; wraps, envelopes and
//! record ciphertexts are synthetic bytes. Synthetic data only.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use vault_proto::backup::stage::{stage, StateInputs};
use vault_proto::crypto::recovery_auth::{self, key_id, RecoveryAuthKey, RecoveryClass};
use vault_proto::crypto::registry::RegistryEntry;
use vault_proto::crypto::secret::{random_secret, SecretBytes};
use vault_proto::handle;
use vault_proto::header::{write_header, Header, Hex16, PasswordWrapFile, RecoveryWrapFile, WrapKdf};
use vault_proto::registry::build;
use vault_proto::registry::chain::{verify_chain_with, EpochPolicy};
use vault_proto::registry::device::{DeviceIdentity, SoftwareDevice, PLATFORM_MACOS};
use vault_proto::registry::file;
use vault_proto::request::{auth_header, Operation, ProviderRequest, SignerId};
use vault_proto::rev::{new_record_id, new_revision_id, RevisionRow};
use vault_proto::state::{RecoveryAuthEntry, StateTransition, TransitionKind};
use vault_provider_core::fs::FsStores;
use vault_provider_core::{Config, Provider, Request, Response};

pub const ORIGIN: &str = "https://provider.test";
pub const T0: u64 = 1_900_000_000;

/// Produces a request signature over its prehash.
pub type Signer<'a> = Box<dyn Fn(&[u8; 32]) -> [u8; 64] + 'a>;

pub enum Who<'a> {
    Dev(&'a SoftwareDevice),
    Rec(&'a RecoveryAuthKey, RecoveryClass),
}

/// The next state a device is about to publish.
pub struct Draft {
    pub header: Header,
    pub registry: Vec<RegistryEntry>,
    pub wrap_mp: Vec<u8>,
    pub wrap_rk: Option<Vec<u8>>,
    pub envs: Vec<([u8; 16], Vec<u8>)>,
    pub revs: Vec<RevisionRow>,
    pub vk: SecretBytes<32>,
    pub updates: Vec<RecoveryAuthEntry>,
}

pub struct Sim {
    pub p: Arc<Provider>,
    pub fs: Arc<FsStores>,
    pub dir: PathBuf,
    pub now: u64,
    pub vault_id: [u8; 16],
    pub handle_key: [u8; 32],
    pub mac: SoftwareDevice,
    pub pk: SecretBytes<32>,
    pub rk: SecretBytes<32>,
    pub cur: Option<Draft>,
    pub generation: u64,
    pub manifest_hash: [u8; 32],
    pub state_commit: [u8; 32],
}

impl Drop for Sim {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn tmp(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("vpc-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn provider_on(fs: Arc<FsStores>) -> Arc<Provider> {
    Arc::new(Provider::with_fs(Config::new(ORIGIN, [0x5e; 32]), fs))
}

pub fn mp_wrap(h: &Header, tag: &str) -> Vec<u8> {
    let w = PasswordWrapFile {
        v: 1,
        kind: "mp".into(),
        kdf_version: 1,
        argon2id: WrapKdf { m: h.kdf.m_kib, t: h.kdf.t, p: h.kdf.p, salt: vault_proto::crypto::hex::encode(h.kdf.salt.0) },
        nonce: "00".repeat(24),
        ct: vault_proto::crypto::hex::encode(format!("synthetic-mp-wrap-{tag}")),
    };
    serde_json::to_vec(&w).unwrap()
}

pub fn rk_wrap(tag: &str) -> Vec<u8> {
    let w = RecoveryWrapFile { v: 1, kind: "rk".into(), salt: "11".repeat(16), nonce: "00".repeat(24), ct: vault_proto::crypto::hex::encode(format!("synthetic-rk-wrap-{tag}")) };
    serde_json::to_vec(&w).unwrap()
}

pub fn env_blob(dev: &[u8; 16], vk_gen: u32) -> Vec<u8> {
    format!("synthetic-envelope-{}-{vk_gen}", vault_proto::crypto::hex::encode(dev)).into_bytes()
}

pub fn row(record_id: &str, author: &SoftwareDevice, counter: u64, parents: Vec<[u8; 32]>, vk_generation: u32) -> RevisionRow {
    RevisionRow {
        revision_id: new_revision_id(),
        record_id: record_id.to_string(),
        parent_ids: parents,
        author_device: vault_proto::rev::uuid_string(&author.device_id()),
        counter,
        deleted: false,
        kind_tag: 1,
        vk_generation,
        schema_version: 1,
        nonce: [3; 24],
        ct: format!("synthetic-ct-{counter}").into_bytes(),
        meta_nonce: [4; 24],
        meta_ct: b"synthetic-meta".to_vec(),
        created_at: T0,
        updated_at: T0,
    }
}

impl Sim {
    pub fn new(tag: &str) -> Sim {
        let dir = tmp(tag);
        let fs = Arc::new(FsStores::new(&dir));
        Sim::on(fs, dir, &format!("synthetic-{tag}@example.test"))
    }

    pub fn on(fs: Arc<FsStores>, dir: PathBuf, handle_text: &str) -> Sim {
        let p = provider_on(fs.clone());
        let mut vid = [0u8; 16];
        getrandom::fill(&mut vid).unwrap();
        Sim {
            p,
            fs,
            dir,
            now: T0,
            vault_id: vid,
            handle_key: handle::handle_key(&handle::normalize(handle_text).unwrap()),
            mac: SoftwareDevice::generate("Synthetic Mac", PLATFORM_MACOS),
            pk: random_secret(),
            rk: random_secret(),
            cur: None,
            generation: 0,
            manifest_hash: [0; 32],
            state_commit: [0; 32],
        }
    }

    pub fn auth_key(&self, class: RecoveryClass, h: &Header) -> RecoveryAuthKey {
        let (secret, salt) = match class {
            RecoveryClass::Mp => (&self.pk, h.auth_salt_mp.0),
            RecoveryClass::Rk => (&self.rk, h.auth_salt_rk.0),
        };
        recovery_auth::derive(class, secret, &salt, &self.vault_id).unwrap()
    }

    pub fn update(&self, class: RecoveryClass, h: &Header) -> RecoveryAuthEntry {
        let k = self.auth_key(class, h);
        let salt = match class {
            RecoveryClass::Mp => h.auth_salt_mp.0,
            RecoveryClass::Rk => h.auth_salt_rk.0,
        };
        RecoveryAuthEntry { class, public: k.public, salt }
    }

    /// The genesis draft (no records).
    pub fn genesis_draft(&self) -> Draft {
        let mut header = Header::fresh(Hex16(self.vault_id), ORIGIN);
        header.vk_generation = 1;
        let registry = vec![build::genesis(&self.mac).unwrap()];
        let updates = vec![self.update(RecoveryClass::Mp, &header), self.update(RecoveryClass::Rk, &header)];
        Draft {
            wrap_mp: mp_wrap(&header, "g"),
            wrap_rk: Some(rk_wrap("g")),
            envs: vec![(self.mac.device_id(), env_blob(&self.mac.device_id(), 1))],
            header,
            registry,
            revs: Vec::new(),
            vk: random_secret(),
            updates,
        }
    }

    /// A draft continuing the committed state (no updates).
    pub fn next_draft(&self) -> Draft {
        let c = self.cur.as_ref().expect("committed state");
        Draft {
            header: c.header.clone(),
            registry: c.registry.clone(),
            wrap_mp: c.wrap_mp.clone(),
            wrap_rk: c.wrap_rk.clone(),
            envs: c.envs.clone(),
            revs: c.revs.clone(),
            vk: SecretBytes::new(*c.vk.expose()),
            updates: Vec::new(),
        }
    }

    /// Stage a draft as the next generation, signed by `signer`.
    pub fn build(&self, d: &Draft, kind: TransitionKind, signer: &dyn DeviceIdentity) -> (StateTransition, Vec<Vec<u8>>) {
        let reg = verify_chain_with(&d.registry, &self.vault_id, &EpochPolicy::CheckpointAnchored).unwrap();
        let staged = stage(
            StateInputs {
                vault_id: self.vault_id,
                generation: self.generation + 1,
                prev_manifest_hash: self.manifest_hash,
                created_at: self.now,
                header: write_header(&d.header).unwrap(),
                registry: file::encode(&d.registry).unwrap(),
                registry_head: reg.head,
                epoch: reg.epoch,
                vk_generation: d.header.vk_generation,
                wrap_mp: d.wrap_mp.clone(),
                wrap_rk: d.wrap_rk.clone(),
                envelopes: d.envs.clone(),
                revisions: &d.revs,
            },
            signer,
            &d.vk,
        )
        .unwrap();
        let t = staged.transition(kind, self.state_commit, d.updates.clone(), Some(self.handle_key));
        (t, staged.blobs.values().cloned().collect())
    }

    pub fn call(&self, op: Operation, blob: Option<&[u8; 32]>, body: &[u8], who: Who<'_>, expected: Option<[u8; 32]>) -> Response {
        let mut n = [0u8; 16];
        getrandom::fill(&mut n).unwrap();
        self.call_n(op, blob, body, who, expected, n)
    }

    pub fn call_n(&self, op: Operation, blob: Option<&[u8; 32]>, body: &[u8], who: Who<'_>, expected: Option<[u8; 32]>, n: [u8; 16]) -> Response {
        let (signer, sign): (SignerId, Signer<'_>) = match who {
            Who::Dev(d) => (
                SignerId::Device { device_id: d.device_id(), key_id: key_id(&d.sign_pub()) },
                Box::new(move |h| d.sign_prehash(h).unwrap()),
            ),
            Who::Rec(k, c) => (SignerId::Recovery { class: c.code(), key_id: k.key_id() }, Box::new(move |h| k.sign_prehash(h))),
        };
        let req = ProviderRequest::build(ORIGIN, self.vault_id, op, blob, signer, body, expected, self.now, n).unwrap();
        let header = auth_header(&req.encode(), &sign(&req.prehash()));
        self.p.handle(&Request { method: &req.method, path: &req.path, auth: Some(&header), body, now: self.now, client_ip: "192.0.2.1" })
    }

    pub fn put_blobs(&self, blobs: &[Vec<u8>], who: &SoftwareDevice) {
        for b in blobs {
            let sha: [u8; 32] = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(b)).into();
            let r = self.call(Operation::BlobPut, Some(&sha), b, Who::Dev(who), None);
            assert_eq!(r.status, 200, "blob_put: {}", String::from_utf8_lossy(&r.body));
        }
    }

    pub fn commit(&self, t: &StateTransition, who: Who<'_>) -> Response {
        let body = t.encode().unwrap();
        self.call(Operation::StateCommit, None, &body, who, Some(t.expected_state))
    }

    /// Record a `200` commit as the new current state.
    pub fn accept(&mut self, r: &Response, d: Draft, t: &StateTransition) {
        assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
        let v: Value = serde_json::from_slice(&r.body).unwrap();
        self.generation = v["generation"].as_u64().unwrap();
        self.state_commit = vault_proto::crypto::hex::decode_array(v["state_commit"].as_str().unwrap()).unwrap();
        self.manifest_hash = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(&t.manifest)).into();
        self.cur = Some(d);
    }

    /// Create the vault; returns the committed sim.
    pub fn created(tag: &str) -> Sim {
        let mut s = Sim::new(tag);
        let d = s.genesis_draft();
        let (t, _) = s.build(&d, TransitionKind::Create, &s.mac);
        let r = s.commit(&t, Who::Dev(&s.mac));
        s.accept(&r, d, &t);
        s
    }

    /// Publish a draft signed by the Mac (blobs uploaded first).
    pub fn publish(&mut self, d: Draft) -> Response {
        let (t, blobs) = self.build(&d, TransitionKind::Publish, &self.mac);
        self.put_blobs(&blobs, &self.mac);
        let r = self.commit(&t, Who::Dev(&self.mac));
        if r.status == 200 {
            self.accept(&r, d, &t);
        }
        r
    }

    pub fn add_record(&self, d: &mut Draft, counter: u64) {
        let vk_gen = d.header.vk_generation;
        d.revs.push(row(&new_record_id(), &self.mac, counter, vec![], vk_gen));
    }

    pub fn error(r: &Response) -> String {
        let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
        v["error"].as_str().unwrap_or("").to_string()
    }
}
