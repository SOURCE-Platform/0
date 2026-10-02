//! Serving the exchange (wire annex A.3.1, A.3.3, A.3.4; PS-12, PW-11):
//! hello's digest, heads and revisions agree on the servable subgraph;
//! closures arrive in the canonical order; local-only revisions are
//! withheld without a fresh provider check; a behind or COMPROMISED Mac
//! refuses what it must. Synthetic data only.

mod device_fx;
mod vault_fx;

use device_fx::*;
use vault_fx::*;
use vault_helper::device::SeDevice;
use vault_helper::peer::verify::{authenticate, reset_rate};
use vault_helper::peer::{ops, Ctx};
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::storage::revisions::uuid_bytes;
use vault_helper::storage::VaultStore;
use vault_proto::backup::object;
use vault_proto::peer::body::{heads_digest, Hello, WITHHELD};
use vault_proto::peer::exchange::{encode_heads_req, encode_revs_get, HeadsItem, HeadsResp, Revs};
use vault_proto::peer::{body_hash, PeerOp, PeerRequest, PeerResponse, PeerStatus};

const NOW: u64 = 1_800_000_000;

struct W {
    fx: Fx,
    _phone: Phone,
    phone_dev: SeDevice,
    id: [u8; 16],
    n: std::cell::Cell<u8>,
}

fn world(tag: &str) -> W {
    let fx = fx();
    setup_and_unlock(&fx);
    let phone = Phone::new(tag);
    let id = enroll_phone(&fx, &phone);
    reset_rate();
    let phone_dev = SeDevice::load_or_create(&std::env::temp_dir().join(format!("vhphone-{}-{}", std::process::id(), tag)), "x", 2).unwrap();
    W { fx, _phone: phone, phone_dev, id, n: std::cell::Cell::new(0) }
}

fn ctx(w: &W, fresh: bool, behind: bool, compromised: bool) -> Ctx {
    let h = VaultStore::read_header(&w.fx.dir).unwrap();
    Ctx { dir: w.fx.dir.clone(), vault_id: h.vault_id.0, me: SeDevice::load(&w.fx.dir).unwrap(), behind, compromised, locked: false, fresh }
}

/// One request through authenticate + serve → (status, body).
fn ask(w: &W, c: &Ctx, op: PeerOp, body: Vec<u8>) -> (PeerStatus, Vec<u8>) {
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

#[test]
fn hello_heads_and_revisions_agree() {
    let _g = serial();
    let w = world("px-agree");
    let r = add_login(&w.fx);
    assert_eq!(w.fx.op(serde_json::json!({"op": "update_item", "ref": r, "title": "second"}))["ok"], true);
    let rid = uuid_bytes(&r).unwrap();
    let c = ctx(&w, true, false, false);

    let (st, b) = ask(&w, &c, PeerOp::Hello, vault_proto::peer::body::empty());
    assert_eq!(st, PeerStatus::Ok);
    let hello = Hello::decode(&b).unwrap();
    let (st, b) = ask(&w, &c, PeerOp::Heads, encode_heads_req(&[vault_proto::peer::body::bucket(&rid)]));
    assert_eq!(st, PeerStatus::Ok);
    let heads = HeadsResp::decode(&b).unwrap();
    assert!(heads.complete);
    let HeadsItem::Heads { heads: h, .. } = heads.items.iter().find(|i| matches!(i, HeadsItem::Heads { record_id, .. } if *record_id == rid)).unwrap().clone() else { unreachable!() };
    assert_eq!(h.len(), 1);
    assert_eq!(hello.heads_digest, heads_digest(&[(rid, h.clone())]), "digest over the same servable heads");

    let (st, b) = ask(&w, &c, PeerOp::RevsGet, encode_revs_get(&[(rid, vec![])]));
    assert_eq!(st, PeerStatus::Ok);
    let revs = Revs::decode(&b, false).unwrap();
    assert_eq!(revs.objects.len(), 2, "both revisions, parents first");
    let rows: Vec<_> = revs.objects.iter().map(|o| object::decode(o).unwrap()).collect();
    assert!(rows[1].parent_ids.contains(&rows[0].revision_id));
    assert_eq!(rows[1].revision_id, h[0]);
    // Holding the head: nothing more to send.
    let (_, b) = ask(&w, &c, PeerOp::RevsGet, encode_revs_get(&[(rid, h)]));
    assert!(Revs::decode(&b, false).unwrap().objects.is_empty());
    w.fx.remove_dir();
}

/// PS-12: without a fresh provider check, local-only revisions are
/// withheld — and said to be, never silently missing.
#[test]
fn local_only_revisions_need_a_fresh_provider_check() {
    let _g = serial();
    let w = world("px-fresh");
    let rid = uuid_bytes(&add_login(&w.fx)).unwrap();
    let stale = ctx(&w, false, false, false);
    let (_, b) = ask(&w, &stale, PeerOp::RevsGet, encode_revs_get(&[(rid, vec![])]));
    let revs = Revs::decode(&b, false).unwrap();
    assert!(revs.objects.is_empty());
    assert_eq!(revs.unavailable, vec![(rid, WITHHELD)]);
    let (_, b) = ask(&w, &stale, PeerOp::Hello, vault_proto::peer::body::empty());
    assert_eq!(Hello::decode(&b).unwrap().heads_digest, heads_digest(&[]), "the digest agrees: nothing servable");
    w.fx.remove_dir();
}

/// PW-11 and the COMPROMISED gate: status 1, hello and status still served.
#[test]
fn a_behind_or_compromised_mac_refuses_what_it_must() {
    let _g = serial();
    let w = world("px-gate");
    let behind = ctx(&w, true, true, false);
    for op in [PeerOp::State, PeerOp::Heads, PeerOp::RevsGet, PeerOp::RevsPut] {
        assert_eq!(ask(&w, &behind, op, vault_proto::peer::body::empty()).0, PeerStatus::BadState, "{op:?}");
    }
    assert_eq!(ask(&w, &behind, PeerOp::Hello, vault_proto::peer::body::empty()).0, PeerStatus::Ok);
    assert_eq!(ask(&w, &behind, PeerOp::Status, vault_proto::peer::body::empty()).0, PeerStatus::Ok);
    let compromised = ctx(&w, true, false, true);
    assert_eq!(ask(&w, &compromised, PeerOp::RevsPut, vault_proto::peer::body::empty()).0, PeerStatus::BadState);
    assert_eq!(ask(&w, &compromised, PeerOp::Heads, encode_heads_req(&[0])).0, PeerStatus::Ok);
    w.fx.remove_dir();
}

/// A malformed body is the signed status 4, nothing applied.
#[test]
fn a_malformed_body_is_status_four() {
    let _g = serial();
    let w = world("px-bad");
    let c = ctx(&w, true, false, false);
    assert_eq!(ask(&w, &c, PeerOp::Heads, vec![0, 0, 0, 0, 1, 0xFF]).0, PeerStatus::FormatInvalid);
    assert_eq!(ask(&w, &c, PeerOp::RevsGet, encode_heads_req(&[1])).0, PeerStatus::FormatInvalid);
    w.fx.remove_dir();
}

/// A copy of the Mac's vault standing in for the phone's store: same key,
/// authoring as the phone's registry id (what a SOURCE Vault phone does).
fn phone_store(w: &W) -> (std::path::PathBuf, VaultStore, vault_helper::crypto::secret::SecretBytes<32>) {
    let dir = std::env::temp_dir().join(format!("vhphone-store-{}-{}", std::process::id(), w.n.get()));
    let _ = std::fs::remove_dir_all(&dir);
    fx_copy(&w.fx.dir, &dir);
    let store = VaultStore::open(&dir).unwrap();
    store.set_author_device(&w.id).unwrap();
    let vk = w.fx.core.lock().unwrap().vk.as_ref().map(|v| vault_helper::crypto::secret::SecretBytes::new(*v.expose())).unwrap();
    (dir, store, vk)
}

fn fx_copy(from: &std::path::Path, to: &std::path::Path) {
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

fn row_of(store: &VaultStore, record: &str) -> vault_helper::storage::revisions::RevisionRow {
    let h = vault_helper::storage::revisions::heads(&store.conn, record).unwrap();
    vault_helper::storage::revisions::get_row(&store.conn, &h[0]).unwrap().unwrap()
}

fn put_body(rows: &[vault_helper::storage::revisions::RevisionRow]) -> Vec<u8> {
    Revs { complete: None, objects: rows.iter().map(|r| object::encode(r).unwrap()).collect(), unavailable: vec![] }.encode()
}

/// PS-01/04 and provenance: a phone's new record is admitted and recorded
/// as delivered by that phone; a forged revision is refused by AEAD; a
/// batch with a missing parent or out of order is refused whole.
#[test]
fn a_phones_revisions_are_opened_before_admission() {
    use vault_helper::storage::sources::{self, Source};
    use vault_proto::peer::body::PutCounts;
    let _g = serial();
    let w = world("px-put");
    let c = ctx(&w, true, false, false);
    let (pdir, mut ps, vk) = phone_store(&w);
    let pt = br#"{"title":"from the phone","username":"p@example.test","password":"synthetic","urls":[{"host":"example.test","match":"exact","allow_http":false}]}"#;
    let meta = br#"{"title":"from the phone","username":"p@example.test","hosts":["example.test"]}"#;
    let rid = ps.add_record(&vk, 1, pt, meta).unwrap();
    let first = row_of(&ps, &rid);

    // Out of order / missing parent: a child alone, its parent unknown here.
    ps.write_successor(&vk, &rid, 1, 1, pt, meta, first.created_at).unwrap();
    let child = row_of(&ps, &rid);
    assert_eq!(ask(&w, &c, PeerOp::RevsPut, put_body(&[child.clone()])).0, PeerStatus::FormatInvalid);
    assert_eq!(ask(&w, &c, PeerOp::RevsPut, put_body(&[child.clone(), first.clone()])).0, PeerStatus::FormatInvalid);

    // A forged revision: garbage ciphertext under a valid-looking row.
    let mut forged = first.clone();
    forged.revision_id = [0x77; 32];
    forged.ct = vec![0u8; first.ct.len()];
    let (st, b) = ask(&w, &c, PeerOp::RevsPut, put_body(&[forged]));
    assert_eq!(st, PeerStatus::Ok);
    assert_eq!(PutCounts::decode(&b).unwrap(), PutCounts { admitted: 0, waiting: 0, refused: 1 });

    // The genuine closure, in canonical order.
    let (st, b) = ask(&w, &c, PeerOp::RevsPut, put_body(&[first.clone(), child.clone()]));
    assert_eq!(st, PeerStatus::Ok);
    assert_eq!(PutCounts::decode(&b).unwrap().admitted, 2);
    let mac = VaultStore::open(&w.fx.dir).unwrap();
    assert_eq!(sources::of(&mac.conn, &child.revision_id).unwrap(), vec![Source::Peer(w.id)]);
    assert!(w.fx.op(serde_json::json!({"op": "list_items"}))["items"].to_string().contains("from the phone"));
    drop(ps);
    let _ = std::fs::remove_dir_all(pdir);
    w.fx.remove_dir();
}
