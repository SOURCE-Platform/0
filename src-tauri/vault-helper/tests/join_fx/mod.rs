//! Shared steps for the phone-join tests: the QR the Mac shows, pairing
//! up to the bundle, streaming it into the phone, and the checks that a
//! refused attempt left nothing behind. Synthetic data only.
#![allow(dead_code)]

use crate::device_fx::{self, FP};
use crate::vault_fx::*;
use serde_json::{json, Value};
use vault_helper::crypto::hex;
use vault_helper::state::VaultState;
use vault_helper::vault::join_ops;
use vault_proto::b64;

pub fn qr(begun: &Value) -> Value {
    json!({
        "v": 2, "host": "192.168.1.20", "port": 50123, "fp": hex::encode(FP),
        "secret": begun["secret"], "mac_device_id": begun["mac_device_id"], "mac_key": begun["mac_key"], "commit": begun["commit"],
        "name": "Synthetic MacBook",
    })
}

/// Stream `bytes` into the phone's join session and complete it.
pub fn complete(phone: &Fx, bytes: &[u8]) -> Value {
    let sha = hex::encode(<[u8; 32]>::from(<sha2::Sha256 as sha2::Digest>::digest(bytes)));
    let begun = phone.op(json!({"op": "join_bundle_begin", "sha256": sha, "size": bytes.len()}));
    let session = begun["session"].as_str().expect("join session").to_string();
    let s = phone.op(json!({"op": "stream_begin", "session": session, "sha256": sha, "size": bytes.len()}));
    let stream = s["stream_id"].as_str().unwrap().to_string();
    for (i, chunk) in bytes.chunks(24 * 1024).enumerate() {
        let w = phone.op(json!({"op": "stream_write", "session": session, "stream_id": stream, "seq": i, "offset": i * 24 * 1024, "data": b64::encode(chunk)}));
        assert_eq!(w["ok"], true, "{w}");
    }
    assert_eq!(phone.op(json!({"op": "stream_end", "session": session, "stream_id": stream}))["ok"], true);
    phone.op(json!({"op": "join_complete", "session": session}))
}

/// QR → hello → the code on both sides → Mac confirm; returns the bundle.
/// `tamper_qr` edits the QR the phone scans.
pub fn pair_up_to_bundle(mac: &Fx, phone: &Fx, tamper_qr: impl FnOnce(&mut Value)) -> (Value, String) {
    let begun = device_fx::begin(mac);
    let mut code = qr(&begun);
    tamper_qr(&mut code);
    let started = phone.op(json!({"op": "join_begin", "qr": code, "name": "Synthetic iPhone"}));
    assert_eq!(started["ok"], true, "{started}");
    let mut hello = started["hello"].clone();
    hello["op"] = json!("enroll_hello");
    let mac_hello = mac.op(hello);
    assert_eq!(mac_hello["ok"], true, "{mac_hello}");
    let phone_side = phone.op(json!({"op": "join_hello", "reply": mac_hello["reply"]}));
    assert_eq!(phone_side["ok"], true, "{phone_side}");
    mac.push_panel(submitted(MP));
    let confirmed = mac.op(json!({"op": "enroll_confirm"}));
    assert_eq!(confirmed["ok"], true, "{confirmed}");
    let mac_code = mac.panel.codes.lock().unwrap().last().cloned().unwrap();
    let mut bundle = confirmed["bundle"].clone();
    // The main app adds this (wire annex A.4).
    bundle["peer_endpoint"] = json!({"spki_sha256": "ab".repeat(32), "token": b64::encode(&[7u8; 32]), "port": 50124, "host_hints": ["192.168.1.20"]});
    let matches = phone_side["sas"].as_str().unwrap() == mac_code;
    (bundle, if matches { "match".into() } else { "differ".into() })
}

pub fn nothing_left(phone: &Fx) {
    assert_eq!(phone.state(), VaultState::Uninitialized);
    for name in [vault_helper::VAULT_HEADER_NAME, vault_helper::device::identity::DEVICE_FILE_NAME, join_ops::ATTEMPT_MARKER] {
        assert!(!phone.dir.join(name).exists(), "{name} left behind");
    }
    assert!(join_ops::stored_peer_endpoint().unwrap().is_none(), "the peer endpoint left behind");
}

/// One pairing whose bundle `tamper` edits; it must be refused, and leave
/// nothing on the phone.
pub fn refused(label: &str, tamper: impl FnOnce(&mut Value)) {
    let _g = serial();
    let mac = fx();
    setup_and_unlock(&mac);
    let phone = fx();
    let (mut bundle, _) = pair_up_to_bundle(&mac, &phone, |_| {});
    tamper(&mut bundle);
    let done = complete(&phone, &serde_json::to_vec(&bundle).unwrap());
    assert_eq!(done["ok"], false, "{label}: {done}");
    nothing_left(&phone);
    mac.remove_dir();
    phone.remove_dir();
}
