//! Serving peer requests (spec v0.5 §22.8, wire annex A.2; PA-01…05,
//! PA-08, PW-07/09): the receiver order, unsigned refusals, who may speak
//! (revoked and pending-revocation senders), the replay cache, the rate
//! limit, and a signed `peer_status` bound to the request. Synthetic.

mod device_fx;
mod vault_fx;

use device_fx::*;
use vault_fx::*;
use vault_helper::device::SeDevice;
use vault_helper::peer::verify::{authenticate, reset_rate};
use vault_helper::peer::{ops, Ctx, Refusal};
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::storage::VaultStore;
use vault_proto::peer::body::{empty, Status};
use vault_proto::peer::{body_hash, PeerOp, PeerRequest, PeerResponse, PeerStatus};

const NOW: u64 = 1_800_000_000;

fn ctx(fx: &Fx) -> Ctx {
    let h = VaultStore::read_header(&fx.dir).unwrap();
    Ctx { dir: fx.dir.clone(), vault_id: h.vault_id.0, me: SeDevice::load(&fx.dir).unwrap(), behind: false, compromised: false, locked: false, fresh: true }
}

/// `id` is the registry id the Mac assigned at enrollment (§5.2).
fn request(c: &Ctx, from: &SeDevice, id: [u8; 16], op: PeerOp, n: u8) -> (Vec<u8>, [u8; 64]) {
    let req = PeerRequest {
        vault_id: c.vault_id,
        sender_device_id: id,
        receiver_device_id: c.me.device_id(),
        operation: op,
        body_sha256: body_hash(&empty()),
        t: NOW,
        n: [n; 16],
    };
    let sig = from.sign_prehash(&req.prehash()).unwrap();
    (req.encode(), sig)
}

fn world(tag: &str) -> (Fx, Phone, [u8; 16], Ctx, VaultStore) {
    let fx = fx();
    setup_and_unlock(&fx);
    let phone = Phone::new(tag);
    let id = enroll_phone(&fx, &phone);
    let c = ctx(&fx);
    let store = VaultStore::open(&fx.dir).unwrap();
    reset_rate();
    (fx, phone, id, c, store)
}

/// A good request is accepted; the signed status binds it.
#[test]
fn a_signed_status_answers_an_enrolled_phone() {
    let _g = serial();
    let (fx, phone, id, c, store) = world("ps-ok");
    let (tlv, sig) = request(&c, &phone.dev, id, PeerOp::Status, 1);
    let acc = authenticate(&c, &store, &tlv, &sig, Some(&empty()), NOW).unwrap();
    let signed = ops::status(&c, &store, &acc, NOW).unwrap();
    let resp = PeerResponse::decode(&signed.response_tlv).unwrap();
    assert_eq!(resp.request_prehash, PeerRequest::decode(&tlv).unwrap().prehash());
    assert_eq!((resp.status, resp.requester_device_id), (PeerStatus::Ok, id));
    assert_eq!(resp.body_sha256, body_hash(&signed.body));
    vault_proto::peer::verify(&resp.prehash(), &signed.signature, &c.me.sign_pub()).expect("signed by the Mac");
    let body = Status::decode(&signed.body).unwrap();
    assert_eq!(body.vault_id, c.vault_id);
    assert_eq!(body.registry, std::fs::read(vault_helper::registry::log::path(&fx.dir)).unwrap());
    fx.remove_dir();
}

/// PA-01/02/03/05: wrong vault or receiver, stale time, replay, a body
/// that does not hash, a foreign signature — all unsigned 403s.
#[test]
fn the_receiver_order_refuses_every_bad_envelope() {
    let _g = serial();
    let (fx, phone, id, c, store) = world("ps-bad");
    let other = Ctx { vault_id: [9; 16], ..ctx(&fx) };
    let (tlv, sig) = request(&other, &phone.dev, id, PeerOp::Hello, 1);
    assert_eq!(authenticate(&c, &store, &tlv, &sig, None, NOW).err(), Some(Refusal::Forbidden), "another vault");
    let mut req = PeerRequest::decode(&request(&c, &phone.dev, id, PeerOp::Hello, 2).0).unwrap();
    req.receiver_device_id = [7; 16];
    let sig = phone.dev.sign_prehash(&req.prehash()).unwrap();
    assert_eq!(authenticate(&c, &store, &req.encode(), &sig, None, NOW).err(), Some(Refusal::Forbidden), "another receiver");
    let (tlv, sig) = request(&c, &phone.dev, id, PeerOp::Hello, 3);
    assert_eq!(authenticate(&c, &store, &tlv, &sig, None, NOW + 301).err(), Some(Refusal::Forbidden), "stale");
    let stranger = Phone::new("ps-stranger");
    let (tlv, bad_sig) = request(&c, &stranger.dev, id, PeerOp::Hello, 4);
    assert_eq!(authenticate(&c, &store, &tlv, &bad_sig, None, NOW).err(), Some(Refusal::Forbidden), "unknown sender");
    let (tlv, sig) = request(&c, &phone.dev, id, PeerOp::Hello, 5);
    assert_eq!(authenticate(&c, &store, &tlv, &sig, Some(b"not the body"), NOW).err(), Some(Refusal::Forbidden), "body hash");
    let (tlv, sig) = request(&c, &phone.dev, id, PeerOp::Hello, 6);
    assert!(authenticate(&c, &store, &tlv, &sig, None, NOW).is_ok());
    assert_eq!(authenticate(&c, &store, &tlv, &sig, None, NOW).err(), Some(Refusal::Forbidden), "replay");
    // PW-07: the replay cache survives a restart (it lives in vault.db).
    let reopened = VaultStore::open(&fx.dir).unwrap();
    assert_eq!(authenticate(&c, &reopened, &tlv, &sig, None, NOW).err(), Some(Refusal::Forbidden), "replay after reopen");
    fx.remove_dir();
}

/// PA-04: a revoked sender gets nothing but `peer_status`.
#[test]
fn a_revoked_phone_may_only_ask_for_its_status() {
    let _g = serial();
    let (fx, phone, id, c, _) = world("ps-revoked");
    fx.push_panel(submitted(MP));
    let r = fx.op(serde_json::json!({"op": "revoke_device", "device_id": vault_helper::crypto::hex::encode(id)}));
    assert_eq!(r["ok"], true, "{r}");
    let store = VaultStore::open(&fx.dir).unwrap();
    let (tlv, sig) = request(&c, &phone.dev, id, PeerOp::RevsGet, 1);
    assert_eq!(authenticate(&c, &store, &tlv, &sig, None, NOW).err(), Some(Refusal::Forbidden));
    let (tlv, sig) = request(&c, &phone.dev, id, PeerOp::Status, 2);
    assert!(authenticate(&c, &store, &tlv, &sig, None, NOW).is_ok(), "status is how it learns");
    // Its revocation is still pending at the provider: a pending target too.
    assert!(vault_helper::sync::pending::revocation_targets(&store.conn).unwrap().contains(&id));
    fx.remove_dir();
}

/// PW-09: the rate limit is unsigned 429, counted after the signature.
#[test]
fn the_rate_limit_counts_only_authenticated_requests() {
    let _g = serial();
    let (fx, phone, id, c, store) = world("ps-rate");
    let stranger = Phone::new("ps-rate-x");
    for i in 0..80u8 {
        let mut req = PeerRequest::decode(&request(&c, &phone.dev, id, PeerOp::Hello, 0).0).unwrap();
        req.n = [i; 16];
        req.n[0] = 0xEE;
        let forged = stranger.dev.sign_prehash(&req.prehash()).unwrap();
        assert_eq!(authenticate(&c, &store, &req.encode(), &forged, None, NOW).err(), Some(Refusal::Forbidden));
    }
    for i in 0..60u8 {
        let (tlv, sig) = request(&c, &phone.dev, id, PeerOp::Hello, i);
        assert!(authenticate(&c, &store, &tlv, &sig, None, NOW).is_ok(), "request {i}");
    }
    let (tlv, sig) = request(&c, &phone.dev, id, PeerOp::Hello, 200);
    assert_eq!(authenticate(&c, &store, &tlv, &sig, None, NOW).err(), Some(Refusal::Rate));
    fx.remove_dir();
}
