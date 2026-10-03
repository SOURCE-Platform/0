//! A phone joins a vault its Mac authorizes (spec §5, v0.5 §22.10): the
//! engine's phone-side ops against the Mac's real enrollment ops, end to
//! end — QR, hello, the code shown by the Mac's Source Vault window, the
//! bundle streamed in, the §22.10 checks, ACK — then the phone unlocks
//! with its own envelope and holds the Mac's records. Every tampered
//! bundle is refused and leaves nothing behind (MT-01). Synthetic data
//! only.

mod device_fx;
mod vault_fx;

use device_fx::FP;
use serde_json::{json, Value};
use vault_fx::*;
use vault_helper::crypto::hex;
use vault_helper::state::VaultState;
use vault_helper::vault::join_ops;
use vault_proto::b64;

fn qr(begun: &Value) -> Value {
    json!({
        "v": 2, "host": "192.168.1.20", "port": 50123, "fp": hex::encode(FP),
        "secret": begun["secret"], "mac_device_id": begun["mac_device_id"], "mac_key": begun["mac_key"],
        "name": "Synthetic MacBook",
    })
}

/// Stream `bytes` into the phone's join session and complete it.
fn complete(phone: &Fx, bytes: &[u8]) -> Value {
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
fn pair_up_to_bundle(mac: &Fx, phone: &Fx, tamper_qr: impl FnOnce(&mut Value)) -> (Value, String) {
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

fn nothing_left(phone: &Fx) {
    assert_eq!(phone.state(), VaultState::Uninitialized);
    for name in [vault_helper::VAULT_HEADER_NAME, vault_helper::device::identity::DEVICE_FILE_NAME, join_ops::ATTEMPT_MARKER] {
        assert!(!phone.dir.join(name).exists(), "{name} left behind");
    }
    assert!(join_ops::stored_peer_endpoint().unwrap().is_none(), "the peer endpoint left behind");
}

/// One pairing whose bundle `tamper` edits; it must be refused, and leave
/// nothing on the phone.
fn refused(label: &str, tamper: impl FnOnce(&mut Value)) {
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

#[test]
fn a_phone_joins_unlocks_and_holds_the_macs_records() {
    let _g = serial();
    let mac = fx();
    setup_and_unlock(&mac);
    let item = add_login(&mac);
    let phone = fx();
    let (bundle, codes) = pair_up_to_bundle(&mac, &phone, |_| {});
    assert_eq!(codes, "match", "the phone and the Source Vault window show the same code");
    let done = complete(&phone, &serde_json::to_vec(&bundle).unwrap());
    assert_eq!(done["ok"], true, "{done}");
    assert_eq!(phone.state(), VaultState::Locked, "materialized, the key not kept");
    // The routes answer only the device that sent the hello (SEC-I3).
    assert_eq!(mac.op(json!({"op": "enroll_proof", "proof": "00".repeat(32)}))["ok"], false);
    assert_eq!(mac.op(json!({"op": "enroll_proof", "proof": done["proof"]}))["ok"], true);
    let acked = mac.op(json!({"op": "enroll_ack", "signature": done["ack"]["signature"]}));
    assert_eq!(acked["ok"], true, "the Mac accepts the phone's ACK: {acked}");
    assert_eq!(phone.op(json!({"op": "join_finish"}))["ok"], true);
    assert_eq!(phone.op(json!({"op": "join_abort"}))["ok"], false, "a finished join cannot be undone this way");
    let ep = join_ops::stored_peer_endpoint().unwrap().expect("peer endpoint in the Keychain");
    assert_eq!(ep["port"], 50124);

    let unlocked = phone.op(json!({"op": "unlock"}));
    assert_eq!(unlocked["ok"], true, "{unlocked}");
    assert!(phone.op(json!({"op": "list_items"}))["items"].to_string().contains(&item));
    assert!(phone.op(json!({"op": "reveal", "ref": item})).to_string().contains(PASSWORD));
    mac.remove_dir();
    phone.remove_dir();
}

/// SEC-B3 (owner decision 2026-10-03): a QR naming another Mac key gives
/// the phone a different code from the Source Vault window's — and the
/// phone refuses the bundle even if the user went on.
#[test]
fn a_qr_with_another_mac_key_shows_a_different_code_and_is_refused() {
    let _g = serial();
    let mac = fx();
    setup_and_unlock(&mac);
    let phone = fx();
    let (bundle, codes) = pair_up_to_bundle(&mac, &phone, |q| q["mac_key"] = json!("cd".repeat(32)));
    assert_eq!(codes, "differ");
    assert_eq!(complete(&phone, &serde_json::to_vec(&bundle).unwrap())["ok"], false);
    nothing_left(&phone);
    mac.remove_dir();
    phone.remove_dir();
}

#[test]
fn a_registry_without_this_phones_entry_is_refused() {
    refused("registry", |b| {
        let entries = vault_helper::registry::file::decode(&hex::decode(b["registry"].as_str().unwrap()).unwrap()).unwrap();
        b["registry"] = json!(hex::encode(vault_helper::registry::file::encode(&entries[..entries.len() - 1]).unwrap()));
    });
}

#[test]
fn another_registry_head_is_refused() {
    refused("head", |b| b["registry_head"] = json!("11".repeat(32)));
}

#[test]
fn a_manifest_with_a_broken_signature_is_refused() {
    refused("manifest", |b| {
        let mut m = hex::decode(b["manifest"].as_str().unwrap()).unwrap();
        let n = m.len() - 3;
        m[n] ^= 0x01;
        b["manifest"] = json!(hex::encode(m));
    });
}

#[test]
fn a_checkpoint_not_under_the_vault_key_is_refused() {
    refused("checkpoint", |b| {
        let mut c = hex::decode(b["checkpoint"].as_str().unwrap()).unwrap();
        let n = c.len() - 3;
        c[n] ^= 0x01;
        b["checkpoint"] = json!(hex::encode(c));
    });
}

/// SEC-B1: an envelope beside the signed index — whatever it seals — is
/// never the one opened.
#[test]
fn an_envelope_other_than_the_indexed_one_is_refused() {
    refused("envelope", |b| {
        let enc = b["envelope"]["enc"].as_str().unwrap().to_string();
        b["envelope"]["enc"] = json!(format!("{}{}", &enc[..enc.len() - 2], if enc.ends_with("00") { "01" } else { "00" }));
    });
}

#[test]
fn an_object_changed_under_its_hash_is_refused() {
    refused("object", |b| {
        let objects = b["objects"].as_array_mut().unwrap();
        let data = objects[0][1].as_str().unwrap().to_string();
        let flipped = if data.starts_with('0') { format!("1{}", &data[1..]) } else { format!("0{}", &data[1..]) };
        objects[0][1] = json!(flipped);
    });
}

#[test]
fn a_bad_peer_endpoint_is_refused() {
    refused("endpoint", |b| b["peer_endpoint"]["token"] = json!("not-a-token"));
}

#[test]
fn a_hello_reply_from_another_mac_is_refused() {
    let _g = serial();
    let mac = fx();
    setup_and_unlock(&mac);
    let phone = fx();
    let begun = device_fx::begin(&mac);
    let started = phone.op(json!({"op": "join_begin", "qr": qr(&begun), "name": "Synthetic iPhone"}));
    let mut hello = started["hello"].clone();
    hello["op"] = json!("enroll_hello");
    let mut reply = mac.op(hello)["reply"].clone();
    reply["mac_device_id"] = json!("ee".repeat(16));
    assert_eq!(phone.op(json!({"op": "join_hello", "reply": reply}))["ok"], false);
    assert_eq!(phone.op(json!({"op": "join_abort"}))["ok"], true);
    nothing_left(&phone);
    mac.remove_dir();
    phone.remove_dir();
}

/// VER-I1: an attempt interrupted after the vault was written (the app
/// killed before the ACK) is removed whole at the next start.
#[test]
fn an_interrupted_join_is_removed_at_the_next_start() {
    let _g = serial();
    let mac = fx();
    setup_and_unlock(&mac);
    let phone = fx();
    let (bundle, _) = pair_up_to_bundle(&mac, &phone, |_| {});
    assert_eq!(complete(&phone, &serde_json::to_vec(&bundle).unwrap())["ok"], true);
    assert!(phone.dir.join(vault_helper::VAULT_HEADER_NAME).exists());
    let rebooted = vault_helper::vault::VaultCore::boot(phone.dir.clone());
    assert_eq!(rebooted.state, VaultState::Uninitialized);
    assert!(!phone.dir.join(vault_helper::VAULT_HEADER_NAME).exists());
    assert!(join_ops::stored_peer_endpoint().unwrap().is_none());
    mac.remove_dir();
    phone.remove_dir();
}
