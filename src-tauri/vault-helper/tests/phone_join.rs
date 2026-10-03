//! A phone joins a vault its Mac authorizes (spec §5, v0.5 §22.10): the
//! engine's phone-side ops against the Mac's real enrollment ops, end to
//! end — QR, hello, SAS on both sides, the bundle streamed in, the §22.10
//! checks, ACK — then the phone unlocks with its own envelope and holds
//! the Mac's records. A bundle that does not enrol this phone's keys is
//! refused and leaves nothing behind. Synthetic data only.

mod device_fx;
mod vault_fx;

use device_fx::FP;
use serde_json::{json, Value};
use vault_fx::*;
use vault_helper::crypto::hex;
use vault_helper::state::VaultState;
use vault_proto::b64;

/// The QR the Mac's screen would show for this enrollment.
fn qr(mac: &Fx) -> Value {
    let begun = device_fx::begin(mac);
    json!({
        "v": 2, "host": "192.168.1.20", "port": 50123, "fp": hex::encode(FP),
        "secret": begun["secret"], "mac_device_id": begun["mac_device_id"], "name": "Synthetic MacBook",
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

/// QR → hello → SAS on both screens → Mac confirm; returns the bundle.
fn pair_up_to_bundle(mac: &Fx, phone: &Fx) -> Value {
    let begun = phone.op(json!({"op": "join_begin", "qr": qr(mac), "name": "Synthetic iPhone"}));
    assert_eq!(begun["ok"], true, "{begun}");
    let mut hello = begun["hello"].clone();
    hello["op"] = json!("enroll_hello");
    let mac_hello = mac.op(hello);
    assert_eq!(mac_hello["ok"], true, "{mac_hello}");
    let phone_sas = phone.op(json!({"op": "join_hello", "reply": mac_hello["reply"]}));
    assert_eq!(phone_sas["sas"], mac_hello["sas"], "both screens show the same code");
    mac.push_panel(submitted(MP));
    let confirmed = mac.op(json!({"op": "enroll_confirm"}));
    assert_eq!(confirmed["ok"], true, "{confirmed}");
    let mut bundle = confirmed["bundle"].clone();
    // The main app adds this (wire annex A.4).
    bundle["peer_endpoint"] = json!({"spki_sha256": "ab".repeat(32), "token": "synthetic-peer-token", "port": 50124, "host_hints": ["192.168.1.20"]});
    bundle
}

#[test]
fn a_phone_joins_unlocks_and_holds_the_macs_records() {
    let _g = serial();
    let mac = fx();
    setup_and_unlock(&mac);
    let item = add_login(&mac);
    let phone = fx();
    assert_eq!(phone.state(), VaultState::Uninitialized);

    let bundle = pair_up_to_bundle(&mac, &phone);
    let done = complete(&phone, &serde_json::to_vec(&bundle).unwrap());
    assert_eq!(done["ok"], true, "{done}");
    assert_eq!(phone.state(), VaultState::Locked, "materialized, the key not kept");
    let acked = mac.op(json!({"op": "enroll_ack", "signature": done["ack"]["signature"]}));
    assert_eq!(acked["ok"], true, "the Mac accepts the phone's ACK: {acked}");
    assert_eq!(phone.op(json!({"op": "join_finish"}))["ok"], true);
    assert_eq!(phone.op(json!({"op": "join_abort"}))["ok"], false, "a finished join cannot be undone this way");

    // The phone opens its own envelope and reads what the Mac wrote.
    let unlocked = phone.op(json!({"op": "unlock"}));
    assert_eq!(unlocked["ok"], true, "{unlocked}");
    let items = phone.op(json!({"op": "list_items"}));
    assert!(items["items"].to_string().contains(&item), "{items}");
    let revealed = phone.op(json!({"op": "reveal", "ref": item}));
    assert!(revealed.to_string().contains(PASSWORD), "{revealed}");
    mac.remove_dir();
    phone.remove_dir();
}

#[test]
fn a_bundle_for_other_keys_is_refused_and_leaves_nothing() {
    let _g = serial();
    let mac = fx();
    setup_and_unlock(&mac);
    let phone = fx();
    let mut bundle = pair_up_to_bundle(&mac, &phone);
    // The head a man in the middle would serve: the registry without the
    // entry that enrols this phone.
    let reg = hex::decode(bundle["registry"].as_str().unwrap()).unwrap();
    let entries = vault_helper::registry::file::decode(&reg).unwrap();
    let shorter = vault_helper::registry::file::encode(&entries[..entries.len() - 1]).unwrap();
    bundle["registry"] = json!(hex::encode(shorter));
    let done = complete(&phone, &serde_json::to_vec(&bundle).unwrap());
    assert_eq!(done["ok"], false, "{done}");
    assert_eq!(phone.state(), VaultState::Uninitialized);
    assert!(!phone.dir.join(vault_helper::VAULT_HEADER_NAME).exists(), "no vault was written");
    assert!(!phone.dir.join(vault_helper::device::identity::DEVICE_FILE_NAME).exists(), "the attempt's keys are gone");
    assert_eq!(mac.op(json!({"op": "cancel_enrollment"}))["ok"], true);
    mac.remove_dir();
    phone.remove_dir();
}
