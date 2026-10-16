//! Phase G (spec §6.5, §6.6, §7.1; design `phase-g-design.md`): a request
//! on the Mac is approved on the paired iPhone over the authenticated peer
//! channel, and only the exact request is released, once. DA-01…DA-09;
//! DA-10/DA-11 (fallback UX, card presence) belong to Phase H's fill path.
//! Synthetic data only.

mod device_fx;
mod join_fx;
mod sync_fx;
mod vault_fx;

use serde_json::{json, Value};
use sync_fx::*;
use vault_fx::*;
use vault_helper::registry::chain::EpochPolicy;
use vault_helper::registry::device::DeviceIdentity;
use vault_proto::approval::{result, Challenge};
use vault_proto::b64;

fn ask(mac: &Fx, origin: &str, item: &str) -> String {
    let r = mac.op(json!({"op": "approval_request", "origin": origin, "action": "fill_password", "credential_ref": item}));
    assert_eq!(r["ok"], true, "{r}");
    r["request_id"].as_str().unwrap().to_string()
}

fn fetch(mac: &Fx, phone: &Fx) -> Value {
    let begun = phone.op(json!({"op": "approvals_fetch_begin"}));
    let req = b64::decode(begun["request"].as_str().unwrap_or_else(|| panic!("{begun}"))).unwrap();
    let answer = relay(mac, &req).expect("served");
    phone.op(json!({"op": "approvals_fetch_step", "response": b64::encode(&answer)}))
}

fn decide(mac: &Fx, phone: &Fx, id: &str, approve: bool) -> Value {
    let d = phone.op(json!({"op": "approval_decide", "request_id": id, "approve": approve}));
    let req = b64::decode(d["request"].as_str().unwrap_or_else(|| panic!("{d}"))).unwrap();
    let answer = relay(mac, &req).expect("served");
    phone.op(json!({"op": "approval_submit_step", "response": b64::encode(&answer)}))
}

fn status(mac: &Fx, id: &str) -> String {
    mac.op(json!({"op": "approval_status", "request_id": id}))["state"].as_str().unwrap_or("").to_string()
}

/// The Mac-side §6.6 check on raw bytes, as the channel would deliver them.
fn check(mac: &Fx, phone: &Fx, bytes: &[u8], now: u64) -> u8 {
    let vid = mac.core.lock().unwrap().header.as_ref().unwrap().vault_id.0;
    let reg = vault_helper::registry::log::read_state(&mac.dir, &vid, &EpochPolicy::CheckpointAnchored).unwrap();
    vault_helper::approval::approve(&reg, device(mac).device_id(), device(phone).device_id(), bytes, now)
}

fn challenge(mac: &Fx, phone: &Fx) -> Challenge {
    vault_helper::approval::challenges_for(device(mac).device_id(), device(phone).device_id(), vault_helper::storage::store::now_epoch()).pop().expect("a pending request")
}

fn sign(phone: &Fx, c: &Challenge) -> Vec<u8> {
    c.signed(&device(phone).sign_prehash(&c.prehash().unwrap()).unwrap()).unwrap()
}

/// DA-01 and the phone's side: it sees exactly the request, approves with
/// presence, and the Mac releases it once.
#[test]
fn da01_an_approved_request_is_released_once() {
    let _g = serial();
    vault_helper::approval::reset();
    let (mac, phone, item) = paired();
    let id = ask(&mac, "https://shop.example.test", &item);
    let list = fetch(&mac, &phone);
    assert_eq!(list["approvals"][0]["origin"], "https://shop.example.test", "{list}");
    assert_eq!(list["approvals"][0]["action"], "fill_password");
    let calls = phone.la.calls.load(std::sync::atomic::Ordering::SeqCst);
    let decided = decide(&mac, &phone, &id, true);
    assert_eq!(decided["result"], "approved", "{decided}");
    assert!(phone.la.calls.load(std::sync::atomic::Ordering::SeqCst) > calls, "presence was asked");
    assert_eq!(status(&mac, &id), "approved");
    assert_eq!(mac.op(json!({"op": "approval_take", "request_id": id}))["ok"], true);
    assert_eq!(mac.op(json!({"op": "approval_take", "request_id": id}))["error"], "APPROVAL_REPLAY");
    mac.remove_dir();
    phone.remove_dir();
}

/// The user's cancel on the phone ends the request.
#[test]
fn a_cancel_on_the_phone_ends_the_request() {
    let _g = serial();
    vault_helper::approval::reset();
    let (mac, phone, item) = paired();
    let id = ask(&mac, "https://shop.example.test", &item);
    fetch(&mac, &phone);
    assert_eq!(decide(&mac, &phone, &id, false)["result"], "cancelled");
    assert_eq!(status(&mac, &id), "cancelled");
    mac.remove_dir();
    phone.remove_dir();
}

/// DA-02…DA-08 on the Mac's §6.6 check.
#[test]
fn da02_to_08_only_the_exact_fresh_approval_passes() {
    let _g = serial();
    vault_helper::approval::reset();
    let (mac, phone, item) = paired();
    ask(&mac, "https://shop.example.test", &item);
    let c = challenge(&mac, &phone);
    let now = c.iat + 1;
    // DA-02: signed by another device (the Mac's own key).
    let by_mac = c.signed(&device(&mac).sign_prehash(&c.prehash().unwrap()).unwrap()).unwrap();
    assert_eq!(check(&mac, &phone, &by_mac, now), result::SIGNATURE);
    // DA-03: an approval naming another Mac.
    let mut other = c.clone();
    other.mac_device_id = [9; 16];
    assert_eq!(check(&mac, &phone, &sign(&phone, &other), now), result::MISMATCH);
    // DA-04: another origin, signed as such (the phishing tripwire).
    let mut phish = c.clone();
    phish.origin = "https://shop.example.test.evil.test".into();
    assert_eq!(check(&mac, &phone, &sign(&phone, &phish), now), result::MISMATCH);
    // DA-08: one bit changed after signing.
    let mut flipped = sign(&phone, &c);
    let at = flipped.windows(4).position(|w| w == b"shop").unwrap();
    flipped[at] ^= 1;
    assert_eq!(check(&mac, &phone, &flipped, now), result::SIGNATURE);
    // DA-05: after `exp`.
    let good = sign(&phone, &c);
    assert_eq!(check(&mac, &phone, &good, c.exp + 1), result::EXPIRED);
    // DA-01 / DA-07: released; the same delivery again is the same answer.
    assert_eq!(check(&mac, &phone, &good, now), result::RELEASED);
    assert_eq!(check(&mac, &phone, &good, now), result::RELEASED, "idempotent");
    // DA-06: once the release was used, the same approval is a replay.
    assert_eq!(mac.op(json!({"op": "approval_take", "request_id": vault_helper::crypto::hex::encode(c.request_id)}))["ok"], true);
    assert_eq!(check(&mac, &phone, &good, now), result::REPLAY);
    mac.remove_dir();
    phone.remove_dir();
}

/// DA-09: a phone revoked meanwhile is not heard at all.
#[test]
fn da09_a_revoked_phone_cannot_approve() {
    let _g = serial();
    vault_helper::approval::reset();
    let (mac, phone, item) = paired();
    let id = ask(&mac, "https://shop.example.test", &item);
    fetch(&mac, &phone);
    let d = phone.op(json!({"op": "approval_decide", "request_id": id, "approve": true}));
    let req = b64::decode(d["request"].as_str().unwrap()).unwrap();
    mac.push_panel(submitted(MP));
    let phone_id = vault_helper::crypto::hex::encode(device(&phone).device_id());
    assert_eq!(mac.op(json!({"op": "revoke_device", "device_id": phone_id}))["ok"], true);
    assert_eq!(relay(&mac, &req), Err(403), "refused before any §6.6 check");
    assert_eq!(status(&mac, &id), "pending");
    mac.remove_dir();
    phone.remove_dir();
}
