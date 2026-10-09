//! The phone's peer exchange against the Mac's real `peer_serve` (spec
//! v0.5 §22.8, §22.9; plan `phase-f2c-phone-plan.md`): a paired phone
//! pulls the Mac's new revisions, admitted with the Mac as source; a
//! second sync has nothing to do; local-only revisions wait for the Mac's
//! freshness; a forged, replayed or swapped answer is "unable to verify"
//! and ends the exchange (PA-05); the Mac removing the phone locks it.
//! Synthetic data only.

mod device_fx;
mod join_fx;
mod vault_fx;

use join_fx::*;
use serde_json::{json, Value};
use vault_fx::*;
use vault_helper::device::SeDevice;
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::state::VaultState;
use vault_helper::vault::peer_serve::Checked;
use vault_proto::b64;
use vault_proto::crypto::tlv::{EntryBuilder, EntryReader};

/// A Mac with one login and a phone paired to it, unlocked.
fn paired() -> (Fx, Fx, String) {
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
/// (§22.14); on real devices each has its own.
fn separate_floors(phone: &Fx) {
    phone.core.lock().unwrap().behind = false;
}

fn fresh(mac: &Fx) {
    mac.core.lock().unwrap().provider_checked = Some(Checked::now());
}

/// The Mac main app's relay: the phone's carriage entry to `peer_serve`,
/// the helper's answer back as a carriage entry (or its refusal).
fn relay(mac: &Fx, request: &str) -> Result<Vec<u8>, u64> {
    let c = b64::decode(request).unwrap();
    let e = EntryReader::parse(&c).unwrap();
    let a = mac.op(json!({"op": "peer_serve", "request_tlv": b64::encode(e.get(1).unwrap()), "signature": b64::encode(e.get(2).unwrap()), "body": b64::encode(e.get(3).unwrap())}));
    if let Some(code) = a.get("refused").and_then(Value::as_u64) {
        return Err(code);
    }
    let field = |k: &str| b64::decode(a[k].as_str().unwrap_or_else(|| panic!("{k} in {a}"))).unwrap();
    Ok(EntryBuilder::new().field_bytes(1, &field("response_tlv")).and_then(|b| b.field_bytes(2, &field("signature"))).and_then(|b| b.field_bytes(3, &field("body"))).unwrap().build())
}

/// One whole exchange; `tamper` may edit each answer before the phone sees it.
fn sync_with(mac: &Fx, phone: &Fx, mut tamper: impl FnMut(usize, Vec<u8>) -> Vec<u8>) -> Value {
    let mut at = phone.op(json!({"op": "peer_sync_begin"}));
    for i in 0.. {
        let Some(request) = at["request"].as_str().map(String::from) else { return at };
        at = match relay(mac, &request) {
            Ok(answer) => phone.op(json!({"op": "peer_sync_step", "response": b64::encode(&tamper(i, answer))})),
            Err(code) => phone.op(json!({"op": "peer_sync_step", "refused": code})),
        };
        assert!(i < 64, "the exchange ends");
    }
    unreachable!()
}

fn sync(mac: &Fx, phone: &Fx) -> Value {
    sync_with(mac, phone, |_, a| a)
}

fn titles(fx: &Fx) -> String {
    fx.op(json!({"op": "list_items"}))["items"].to_string()
}

#[test]
fn a_phone_pulls_the_macs_new_items() {
    let _g = serial();
    let (mac, phone, _) = paired();
    let r = mac.op(json!({"op": "add_item", "kind": "login", "title": "Added After Pairing", "username": "later@example.test", "hosts": ["later.example.test"], "password": PASSWORD2}));
    assert_eq!(r["ok"], true, "{r}");
    let new_ref = r["ref"].as_str().unwrap().to_string();
    fresh(&mac);
    assert!(!titles(&phone).contains("Added After Pairing"));
    let done = sync(&mac, &phone);
    assert_eq!(done["ok"], true, "{done}");
    assert!(done["done"]["admitted"].as_u64().unwrap() >= 1, "{done}");
    assert!(titles(&phone).contains("Added After Pairing"));
    assert!(phone.op(json!({"op": "reveal", "ref": new_ref})).to_string().contains(PASSWORD2));
    // Nothing differs any more: the hello digests agree.
    let again = sync(&mac, &phone);
    assert_eq!(again["done"]["admitted"], 0, "{again}");
    mac.remove_dir();
    phone.remove_dir();
}

#[test]
fn local_only_revisions_wait_for_the_macs_freshness() {
    let _g = serial();
    let (mac, phone, _) = paired();
    assert_eq!(mac.op(json!({"op": "add_item", "kind": "login", "title": "Not Yet Confirmed", "username": "u@example.test", "hosts": ["example.test"], "password": PASSWORD2}))["ok"], true);
    // No verified provider exchange in the last 15 minutes (§22.7).
    let done = sync(&mac, &phone);
    assert_eq!(done["ok"], true, "{done}");
    assert!(!titles(&phone).contains("Not Yet Confirmed"));
    mac.remove_dir();
    phone.remove_dir();
}

/// PA-05, requester side: every altered answer is "unable to verify", and
/// the exchange ends with nothing applied.
#[test]
fn altered_answers_are_unable_to_verify() {
    let _g = serial();
    let (mac, phone, _) = paired();
    assert_eq!(mac.op(json!({"op": "add_item", "kind": "login", "title": "Should Not Arrive", "username": "u@example.test", "hosts": ["example.test"], "password": PASSWORD2}))["ok"], true);
    fresh(&mac);
    let swap_body = |a: Vec<u8>| {
        let e = EntryReader::parse(&a).unwrap();
        EntryBuilder::new().field_bytes(1, e.get(1).unwrap()).and_then(|b| b.field_bytes(2, e.get(2).unwrap())).and_then(|b| b.field_bytes(3, &vault_proto::peer::body::empty())).unwrap().build()
    };
    let flip_sig = |a: Vec<u8>| {
        let e = EntryReader::parse(&a).unwrap();
        let mut sig = e.get(2).unwrap().to_vec();
        sig[10] ^= 1;
        EntryBuilder::new().field_bytes(1, e.get(1).unwrap()).and_then(|b| b.field_bytes(2, &sig)).and_then(|b| b.field_bytes(3, e.get(3).unwrap())).unwrap().build()
    };
    let mut first: Option<Vec<u8>> = None;
    let replay = |i: usize, a: Vec<u8>| {
        // The status answer, replayed for the next request.
        if i == 0 {
            first = Some(a.clone());
            a
        } else {
            first.clone().unwrap()
        }
    };
    for (label, out) in [
        ("swapped body", sync_with(&mac, &phone, |_, a| swap_body(a))),
        ("flipped signature", sync_with(&mac, &phone, |i, a| if i == 1 { flip_sig(a) } else { a })),
        ("replayed answer", sync_with(&mac, &phone, replay)),
    ] {
        assert_eq!(out["error"], "PEER_AUTH_INVALID", "{label}: {out}");
        assert!(!titles(&phone).contains("Should Not Arrive"), "{label}");
        assert_eq!(phone.op(json!({"op": "peer_sync_step", "response": "AA"}))["error"], "BAD_STATE", "{label}: the exchange ended");
    }
    mac.remove_dir();
    phone.remove_dir();
}

/// §22.9: the Mac removes the phone; a verified `peer_status` locks it,
/// records the removal, and stops authoring and further exchanges.
#[test]
fn a_removal_from_the_mac_locks_the_phone() {
    let _g = serial();
    let (mac, phone, _) = paired();
    let phone_id = SeDevice::load(&phone.dir).unwrap().device_id();
    mac.push_panel(submitted(MP)); // the revocation's rotation asks for the master password
    let r = mac.op(json!({"op": "revoke_device", "device_id": vault_helper::crypto::hex::encode(phone_id)}));
    assert_eq!(r["ok"], true, "{r}");
    let out = sync(&mac, &phone);
    assert_eq!(out["removed"]["published"], false, "no provider here: still among the Mac's own entries: {out}");
    assert_eq!(phone.state(), VaultState::Locked);
    let state = phone.core.lock().unwrap().state_answer();
    assert_eq!(state["removed"]["published"], false, "{state}");
    assert_eq!(phone.op(json!({"op": "unlock"}))["ok"], true, "reads stay possible: nothing is deleted");
    separate_floors(&phone);
    assert_eq!(phone.op(json!({"op": "peer_sync_begin"}))["error"], "PEER_NOT_PERMITTED");
    let add = phone.op(json!({"op": "add_item", "kind": "login", "title": "x", "username": "u", "hosts": ["example.test"], "password": "p"}));
    assert_eq!(add["error"], "PEER_NOT_PERMITTED");
    mac.remove_dir();
    phone.remove_dir();
}
