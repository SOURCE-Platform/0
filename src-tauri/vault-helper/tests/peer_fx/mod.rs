//! Shared fixture for the peer-serving tests: an enrolled synthetic
//! phone, a serving context, one-call request/serve, and a phone-side
//! store that authors as the phone. Synthetic data only.
#![allow(dead_code)]

use crate::device_fx::*;
use crate::vault_fx::*;
use vault_helper::device::SeDevice;
use vault_helper::peer::verify::{authenticate, reset_rate};
use vault_helper::peer::{ops, Ctx};
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::storage::VaultStore;
use vault_proto::backup::object;
use vault_proto::peer::exchange::Revs;
use vault_proto::peer::{body_hash, PeerOp, PeerRequest, PeerResponse, PeerStatus};

pub const NOW: u64 = 1_800_000_000;

pub struct W {
    pub fx: Fx,
    pub _phone: Phone,
    pub phone_dev: SeDevice,
    pub id: [u8; 16],
    pub n: std::cell::Cell<u8>,
}

pub fn world(tag: &str) -> W {
    let fx = fx();
    setup_and_unlock(&fx);
    let phone = Phone::new(tag);
    let id = enroll_phone(&fx, &phone);
    reset_rate();
    let phone_dev = SeDevice::load_or_create(&std::env::temp_dir().join(format!("vhphone-{}-{}", std::process::id(), tag)), "x", 2).unwrap();
    W { fx, _phone: phone, phone_dev, id, n: std::cell::Cell::new(0) }
}

pub fn ctx(w: &W, fresh: bool, behind: bool, compromised: bool) -> Ctx {
    let h = VaultStore::read_header(&w.fx.dir).unwrap();
    Ctx { dir: w.fx.dir.clone(), vault_id: h.vault_id.0, me: SeDevice::load(&w.fx.dir).unwrap(), behind, compromised, locked: false, fresh }
}

/// One request through authenticate + serve → (status, body).
pub fn ask(w: &W, c: &Ctx, op: PeerOp, body: Vec<u8>) -> (PeerStatus, Vec<u8>) {
    w.n.set(w.n.get() + 1);
    let req = PeerRequest {
        vault_id: c.vault_id,
        sender_device_id: w.id,
        receiver_device_id: c.me.device_id(),
        operation: op,
        body_sha256: body_hash(&body),
        t: NOW,
        n: [w.n.get(); 16],
    };
    let sig = w.phone_dev.sign_prehash(&req.prehash()).unwrap();
    let mut store = VaultStore::open(&w.fx.dir).unwrap();
    let acc = authenticate(c, &store, &req.encode(), &sig, Some(&body), NOW).expect("authenticated");
    let vk = w.fx.core.lock().unwrap().vk.as_ref().map(|v| vault_helper::crypto::secret::SecretBytes::new(*v.expose()));
    let signed = ops::serve(c, &mut store, vk.as_ref(), &acc, &body, NOW).expect("served");
    let resp = PeerResponse::decode(&signed.response_tlv).unwrap();
    assert_eq!(resp.body_sha256, body_hash(&signed.body));
    (resp.status, signed.body)
}

/// A copy of the Mac's vault standing in for the phone's store: same key,
/// authoring as the phone's registry id (what a SOURCE Vault phone does).
pub fn phone_store(w: &W) -> (std::path::PathBuf, VaultStore, vault_helper::crypto::secret::SecretBytes<32>) {
    let dir = std::env::temp_dir().join(format!("vhphone-store-{}-{}", std::process::id(), w.n.get()));
    let _ = std::fs::remove_dir_all(&dir);
    fx_copy(&w.fx.dir, &dir);
    let store = VaultStore::open(&dir).unwrap();
    store.set_author_device(&w.id).unwrap();
    let vk = w.fx.core.lock().unwrap().vk.as_ref().map(|v| vault_helper::crypto::secret::SecretBytes::new(*v.expose())).unwrap();
    (dir, store, vk)
}

pub fn fx_copy(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let dst = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            fx_copy(&e.path(), &dst);
        } else if !e.file_name().to_string_lossy().ends_with(".sock") {
            std::fs::copy(e.path(), dst).unwrap();
        }
    }
}

pub fn row_of(store: &VaultStore, record: &str) -> vault_helper::storage::revisions::RevisionRow {
    let h = vault_helper::storage::revisions::heads(&store.conn, record).unwrap();
    vault_helper::storage::revisions::get_row(&store.conn, &h[0]).unwrap().unwrap()
}

pub fn put_body(rows: &[vault_helper::storage::revisions::RevisionRow]) -> Vec<u8> {
    Revs { complete: None, objects: rows.iter().map(|r| object::encode(r).unwrap()).collect(), unavailable: vec![] }.encode()
}

