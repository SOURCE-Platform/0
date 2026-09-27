//! Total-loss recovery driver for the simulator (spec v0.4 §11.8).

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
