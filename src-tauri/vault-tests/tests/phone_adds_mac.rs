//! F.2e reverse enrollment (spec §22.13; design `phase-f2e-design.md`
//! revision 2), in process: an iPhone engine (the C ABI) adds a fresh Mac
//! helper (the op dispatcher) after the lost Mac was revoked. The frames
//! the two would carry over TLS are passed directly. Synthetic data only.

#[path = "../../vault-helper/tests/vault_fx/mod.rs"]
mod vault_fx;
mod mfx;
mod phone_fx;

use mfx::*;
use phone_fx::*;
use serde_json::{json, Value};
use vault_ffi::*;
use vault_helper::registry::device::DeviceIdentity;

fn endpoint() -> Value {
    json!({ "spki_sha256": "ab".repeat(32), "token": vault_proto::b64::encode(&[7u8; 32]), "port": 4500, "host_hints": ["192.168.1.20"] })
}

/// The phone, paired with Mac `a`, has revoked it (published) and is fresh.
fn phone_without_its_mac(tag: &str) -> (Box<Ctx>, Mac, Mac, *const Ov0Engine) {
    let (cloud, a, b) = pair(tag);
    let e = open(&cloud, &b.dir);
    assert_eq!(call(e, r#"{"op":"provider_sync"}"#)["ok"], true);
    let added = call(e, r#"{"op":"add_item","kind":"login","title":"kept-on-the-phone","username":"u@example.test","hosts":["example.test"],"password":"synthetic-z"}"#);
    assert_eq!(added["ok"], true, "{added}");
    let mac = vault_helper::crypto::hex::encode(a.dev.device_id());
    assert_eq!(call(e, &format!(r#"{{"op":"revoke_device","device_id":"{mac}"}}"#))["ok"], true);
    assert_eq!(call(e, r#"{"op":"provider_publish"}"#)["result"]["committed"], true);
    assert_eq!(call(e, r#"{"op":"provider_sync"}"#)["ok"], true, "fresh");
    (cloud, a, b, e)
}

fn qr(begun: &Value) -> String {
    json!({ "v": 3, "host": "192.168.1.20", "port": 4500, "fp": "cd".repeat(32), "secret": begun["secret"], "commit": begun["commit"] }).to_string()
}

/// Read the phone's bundle out of the engine, stream it into the Mac.
fn carry_bundle(e: *const Ov0Engine, mac: &vault_fx::Fx, confirmed: &Value) -> String {
    let (session, sha, size) = (confirmed["session"].as_str().unwrap(), confirmed["stream"].as_str().unwrap(), confirmed["size"].as_u64().unwrap());
    let mut bytes = Vec::new();
    while (bytes.len() as u64) < size {
        let r = call(e, &json!({"op": "stream_read", "session": session, "sha256": sha, "offset": bytes.len()}).to_string());
        bytes.extend(vault_proto::b64::decode(r["data"].as_str().unwrap()).unwrap());
    }
    let s = mac.op(json!({"op": "renroll_bundle_begin", "sha256": sha, "size": size}));
    let mac_session = s["session"].as_str().expect("bundle session").to_string();
    let st = mac.op(json!({"op": "stream_begin", "session": mac_session, "sha256": sha, "size": size}));
    let stream = st["stream_id"].as_str().unwrap().to_string();
    for (seq, chunk) in bytes.chunks(48 * 1024).enumerate() {
        let w = mac.op(json!({"op": "stream_write", "session": mac_session, "stream_id": stream, "seq": seq, "offset": seq * 48 * 1024, "data": vault_proto::b64::encode(chunk)}));
        assert_eq!(w["ok"], true, "{w}");
    }
    assert_eq!(mac.op(json!({"op": "stream_end", "session": mac_session, "stream_id": stream}))["ok"], true);
    mac_session
}

/// RE-01 (with RE-02's code agreement): the whole flow; the new Mac opens
/// with the master password and holds the phone's items.
#[test]
fn an_iphone_adds_a_replacement_mac() {
    let _g = serial();
    let mac = vault_fx::fx(); // first: it resets the test Keychain
    let (cloud, _a, b, e) = phone_without_its_mac("re-01");
    let begun = mac.op(json!({"op": "renroll_begin", "fp": "cd".repeat(32), "name": "Synthetic New Mac"}));
    assert!(begun.get("nonce_n").is_none() && begun["commit"].is_string(), "no nonce leaves the helper: {begun}");
    let start = call(e, &json!({"op": "authorize_begin", "qr": serde_json::from_str::<Value>(&qr(&begun)).unwrap()}).to_string());
    assert_eq!(start["ok"], true, "{start}");
    let reply = mac.op(json!({"op": "renroll_hello", "hello": start["hello"]}));
    let code = call(e, &json!({"op": "authorize_reply", "reply": reply["reply"]}).to_string());
    assert_eq!(code["ok"], true, "{code}");
    mac.push_panel(vault_fx::submitted(MP));
    assert_eq!(mac.op(json!({"op": "renroll_confirm"}))["ok"], true);
    assert_eq!(mac.panel.codes.lock().unwrap().last().map(String::as_str), code["sas"].as_str(), "both screens show one code");
    let confirmed = call(e, r#"{"op":"authorize_confirm"}"#);
    assert_eq!(confirmed["ok"], true, "{confirmed}");
    assert_eq!(mac.op(json!({"op": "renroll_proof", "proof": confirmed["proof"]}))["ok"], true);
    let session = carry_bundle(e, &mac, &confirmed);
    let acked = mac.op(json!({"op": "renroll_complete", "session": session}));
    assert_eq!(acked["ok"], true, "{acked}");
    assert!(!mac.dir.join("vault.db").exists(), "nothing in place before done");
    let finished = call(e, &json!({"op": "authorize_finish", "ack": acked["ack"], "peer_endpoint": endpoint()}).to_string());
    assert_eq!(finished["ok"], true, "{finished}");
    assert_eq!(mac.op(json!({"op": "renroll_proof", "proof": finished["proof"]}))["ok"], true);
    assert_eq!(mac.op(json!({"op": "renroll_finish", "registry_head": finished["registry_head"]}))["ok"], true);
    assert_eq!(vault_fx::unlock(&mac, MP)["ok"], true, "the new Mac opens with the master password");
    assert!(mac.op(json!({"op": "list_items"})).to_string().contains("kept-on-the-phone"));
    let devices = call(e, r#"{"op":"list_devices"}"#);
    assert!(devices["devices"].as_array().unwrap().iter().any(|d| d["device_name"] == "Synthetic New Mac" && d["revoked"] == false), "{devices}");
    assert_eq!(call(e, r#"{"op":"provider_publish"}"#)["result"]["committed"], true, "provider activation published");
    let _ = (cloud, b);
    unsafe { ov0_engine_close(e) };
    mac.remove_dir();
}

/// RE-06: replacement only, and fresh — refused before any prompt.
/// RE-11: a QR from a public host is refused, a forward-pairing (v:2)
/// QR is not a reverse one, and the route proof is required.
#[test]
fn only_a_replacement_and_only_when_fresh() {
    let _g = serial();
    let mac = vault_fx::fx();
    let (cloud, _a, b) = pair("re-06");
    let e = open(&cloud, &b.dir);
    assert_eq!(call(e, r#"{"op":"provider_sync"}"#)["ok"], true);
    let begun = mac.op(json!({"op": "renroll_begin", "fp": "cd".repeat(32)}));
    let frame = json!({"op": "authorize_begin", "qr": serde_json::from_str::<Value>(&qr(&begun)).unwrap()}).to_string();
    assert_eq!(call(e, &frame)["error"], "REPLACEMENT_ONLY", "Mac a is still active");
    let public = frame.replace("192.168.1.20", "8.8.8.8");
    assert_eq!(call(e, &public)["error"], "INVALID_INPUT", "a QR from afar is refused");
    let forward = frame.replace("\"v\":3", "\"v\":2");
    assert_eq!(call(e, &forward)["error"], "PROTOCOL_VIOLATION", "a v:2 QR is not a reverse enrollment");
    assert_eq!(mac.op(json!({"op": "renroll_proof", "proof": "00".repeat(32)}))["error"], "WRONG_CREDENTIAL", "no route without the proof");
    unsafe { ov0_engine_close(e) };
    mac.remove_dir();
}

/// RE-03 / RE-09: the Mac keeps nothing without the panel; a wrong master
/// password keeps nothing either.
#[test]
fn the_mac_keeps_nothing_without_the_panel_or_with_a_wrong_password() {
    let _g = serial();
    let mac = vault_fx::fx();
    let (_cloud, _a, _b, e) = phone_without_its_mac("re-03");
    let begun = mac.op(json!({"op": "renroll_begin", "fp": "cd".repeat(32)}));
    let start = call(e, &json!({"op": "authorize_begin", "qr": serde_json::from_str::<Value>(&qr(&begun)).unwrap()}).to_string());
    let reply = mac.op(json!({"op": "renroll_hello", "hello": start["hello"]}));
    assert_eq!(call(e, &json!({"op": "authorize_reply", "reply": reply["reply"]}).to_string())["ok"], true);
    assert_eq!(mac.op(json!({"op": "renroll_bundle_begin", "sha256": "00".repeat(32), "size": 10}))["error"], "BAD_STATE", "no bundle before Continue");
    mac.push_panel(vault_fx::submitted(b"not-the-master-password"));
    assert_eq!(mac.op(json!({"op": "renroll_confirm"}))["ok"], true);
    let confirmed = call(e, r#"{"op":"authorize_confirm"}"#);
    let session = carry_bundle(e, &mac, &confirmed);
    assert_eq!(mac.op(json!({"op": "renroll_complete", "session": session}))["error"], "WRONG_CREDENTIAL");
    assert!(!mac.dir.join("renroll").exists() && !mac.dir.join("vault.db").exists(), "the attempt is gone");
    unsafe { ov0_engine_close(e) };
    mac.remove_dir();
}

/// RE-10: a vault recovered onto a phone (every other device lost, §12
/// scenario 3) adds a replacement Mac, which opens with the master
/// password and holds the vault's items.
#[test]
fn a_phone_recovered_vault_adds_a_mac() {
    let _g = serial();
    let mac = vault_fx::fx();
    let cloud = ctx("re-10");
    let mut old = Mac::new("re-10-old");
    old.setup(&cloud.cloud, "synthetic-re10@example.test").unwrap();
    old.add("from-before-the-loss");
    old.publish(&cloud.cloud).unwrap();
    let e = open_raw(&cloud, &tmp("re-10-phone"));
    assert_eq!(call(e, r#"{"op":"provider_recover_start","handle":"synthetic-re10@example.test","kind":"mp"}"#)["ok"], true);
    assert_eq!(call(e, r#"{"op":"recovery_preview"}"#)["ok"], true);
    assert_eq!(call(e, r#"{"op":"provider_recover_finish"}"#)["ok"], true);
    assert_eq!(call(e, r#"{"op":"provider_sync"}"#)["ok"], true, "fresh");
    let begun = mac.op(json!({"op": "renroll_begin", "fp": "cd".repeat(32), "name": "Synthetic Replacement Mac"}));
    let start = call(e, &json!({"op": "authorize_begin", "qr": serde_json::from_str::<Value>(&qr(&begun)).unwrap()}).to_string());
    assert_eq!(start["ok"], true, "{start}");
    let reply = mac.op(json!({"op": "renroll_hello", "hello": start["hello"]}));
    assert_eq!(call(e, &json!({"op": "authorize_reply", "reply": reply["reply"]}).to_string())["ok"], true);
    mac.push_panel(vault_fx::submitted(MP));
    assert_eq!(mac.op(json!({"op": "renroll_confirm"}))["ok"], true);
    let confirmed = call(e, r#"{"op":"authorize_confirm"}"#);
    assert_eq!(confirmed["ok"], true, "{confirmed}");
    let session = carry_bundle(e, &mac, &confirmed);
    let acked = mac.op(json!({"op": "renroll_complete", "session": session}));
    assert_eq!(acked["ok"], true, "{acked}");
    let finished = call(e, &json!({"op": "authorize_finish", "ack": acked["ack"], "peer_endpoint": endpoint()}).to_string());
    assert_eq!(finished["ok"], true, "{finished}");
    assert_eq!(mac.op(json!({"op": "renroll_finish", "registry_head": finished["registry_head"]}))["ok"], true);
    assert_eq!(vault_fx::unlock(&mac, MP)["ok"], true);
    assert!(mac.op(json!({"op": "list_items"})).to_string().contains("from-before-the-loss"));
    unsafe { ov0_engine_close(e) };
    mac.remove_dir();
}
