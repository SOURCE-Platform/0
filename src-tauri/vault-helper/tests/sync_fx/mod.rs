//! Shared steps for the phone-sync tests: a Mac and a paired phone, the
//! main app's relay (inline and streamed answers, both directions), whole
//! exchanges with an optional tamper hook, and a misbehaving Mac helper
//! that re-signs an edited answer with its own key. Synthetic data only.
#![allow(dead_code)]

use crate::join_fx::*;
use crate::vault_fx::*;
use serde_json::{json, Value};
use vault_helper::device::SeDevice;
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::vault::peer_serve::Checked;
use vault_proto::b64;
use vault_proto::crypto::tlv::{EntryBuilder, EntryReader};
use vault_proto::peer::{body_hash, PeerResponse};

/// A Mac with one login and a phone paired to it, unlocked.
pub fn paired() -> (Fx, Fx, String) {
    let mac = fx();
    setup_and_unlock(&mac);
    let item = add_login(&mac);
    let phone = fx();
    let (bundle, _) = pair_up_to_bundle(&mac, &phone, |_| {});
    let done = complete(&phone, &serde_json::to_vec(&bundle).unwrap());
    assert_eq!(mac.op(json!({"op": "enroll_proof", "proof": done["proof"]}))["ok"], true);
    assert_eq!(mac.op(json!({"op": "enroll_ack", "signature": done["ack"]["signature"]}))["ok"], true);
    assert_eq!(phone.op(json!({"op": "join_finish"}))["ok"], true);
    assert_eq!(phone.op(json!({"op": "unlock"}))["ok"], true);
    separate_floors(&phone);
    (mac, phone, item)
}

/// Both fixtures share one test Keychain in this process, so the phone
/// reads the Mac's newer rollback floor and believes itself behind
/// (§22.14); on real devices each has its own (verified in review: with
/// its own floor restored, a freshly joined phone is not behind).
pub fn separate_floors(phone: &Fx) {
    phone.core.lock().unwrap().behind = false;
}

pub fn fresh(mac: &Fx) {
    mac.core.lock().unwrap().provider_checked = Some(Checked::now());
}

pub fn add(fx: &Fx, title: &str) -> String {
    let r = fx.op(json!({"op": "add_item", "kind": "login", "title": title, "username": "u@example.test", "hosts": ["example.test"], "password": PASSWORD2}));
    assert_eq!(r["ok"], true, "{r}");
    r["ref"].as_str().unwrap().to_string()
}

pub fn titles(fx: &Fx) -> String {
    fx.op(json!({"op": "list_items"}))["items"].to_string()
}

/// The Mac main app's relay: the phone's carriage entry to `peer_serve`,
/// the helper's answer back as a carriage entry — a large answer read
/// through its stream session — or its refusal.
pub fn relay(mac: &Fx, request: &[u8]) -> Result<Vec<u8>, u64> {
    let e = EntryReader::parse(request).unwrap();
    let a = mac.op(json!({"op": "peer_serve", "request_tlv": b64::encode(e.get(1).unwrap()), "signature": b64::encode(e.get(2).unwrap()), "body": b64::encode(e.get(3).unwrap())}));
    if let Some(code) = a.get("refused").and_then(Value::as_u64) {
        return Err(code);
    }
    let field = |k: &str| b64::decode(a[k].as_str().unwrap_or_else(|| panic!("{k} in {a}"))).unwrap();
    let body = match a.get("session").and_then(Value::as_str) {
        None => field("body"),
        Some(session) => {
            let size = a["size"].as_u64().unwrap() as usize;
            let mut got = Vec::new();
            while got.len() < size {
                let chunk = mac.op(json!({"op": "stream_read", "session": session, "sha256": a["stream"], "offset": got.len()}));
                assert_eq!(chunk["ok"], true, "{chunk}");
                got.extend(b64::decode(chunk["data"].as_str().unwrap()).unwrap());
            }
            mac.op(json!({"op": "session_close", "session": session}));
            got
        }
    };
    Ok(carriage(&field("response_tlv"), &field("signature"), &body))
}

pub fn carriage(tlv: &[u8], sig: &[u8], body: &[u8]) -> Vec<u8> {
    EntryBuilder::new().field_bytes(1, tlv).and_then(|b| b.field_bytes(2, sig)).and_then(|b| b.field_bytes(3, body)).unwrap().build()
}

/// What SOURCE Vault does with one answer: inline when it fits one FFI
/// frame, otherwise streamed in through `peer_sync_receive` (SEC-I1).
pub fn deliver(phone: &Fx, answer: &[u8]) -> Value {
    if answer.len() <= 40 * 1024 {
        return phone.op(json!({"op": "peer_sync_step", "response": b64::encode(answer)}));
    }
    let sha = vault_helper::crypto::hex::encode(body_hash(answer));
    let r = phone.op(json!({"op": "peer_sync_receive", "sha256": sha, "size": answer.len()}));
    let session = r["session"].as_str().unwrap_or_else(|| panic!("{r}")).to_string();
    let s = phone.op(json!({"op": "stream_begin", "session": session, "sha256": sha, "size": answer.len()}));
    let stream = s["stream_id"].as_str().unwrap().to_string();
    for (i, chunk) in answer.chunks(24 * 1024).enumerate() {
        let w = phone.op(json!({"op": "stream_write", "session": session, "stream_id": stream, "seq": i, "offset": i * 24 * 1024, "data": b64::encode(chunk)}));
        assert_eq!(w["ok"], true, "{w}");
    }
    assert_eq!(phone.op(json!({"op": "stream_end", "session": session, "stream_id": stream}))["ok"], true);
    phone.op(json!({"op": "peer_sync_step", "session": session}))
}

/// One whole exchange; `tamper(step, answer)` may edit each answer.
pub fn sync_with(mac: &Fx, phone: &Fx, mut tamper: impl FnMut(usize, Vec<u8>) -> Vec<u8>) -> Value {
    let mut at = phone.op(json!({"op": "peer_sync_begin"}));
    for i in 0..64 {
        let Some(request) = at["request"].as_str().map(|r| b64::decode(r).unwrap()) else { return at };
        at = match relay(mac, &request) {
            Ok(answer) => deliver(phone, &tamper(i, answer)),
            Err(code) => phone.op(json!({"op": "peer_sync_step", "refused": code})),
        };
    }
    panic!("the exchange did not end: {at}")
}

pub fn sync(mac: &Fx, phone: &Fx) -> Value {
    sync_with(mac, phone, |_, a| a)
}

/// A misbehaving signer: edit the answer's envelope and body, then sign it
/// with `signer`'s key (the Mac's own, for a helper that lies).
pub fn resign(signer: &SeDevice, answer: &[u8], edit: impl FnOnce(&mut PeerResponse, &mut Vec<u8>)) -> Vec<u8> {
    let e = EntryReader::parse(answer).unwrap();
    let mut resp = PeerResponse::decode(e.get(1).unwrap()).unwrap();
    let mut body = e.get(3).unwrap().to_vec();
    edit(&mut resp, &mut body);
    resp.body_sha256 = body_hash(&body);
    let sig = signer.sign_prehash(&resp.prehash()).unwrap();
    carriage(&resp.encode(), &sig, &body)
}

pub fn device(fx: &Fx) -> SeDevice {
    SeDevice::load(&fx.dir).unwrap()
}
