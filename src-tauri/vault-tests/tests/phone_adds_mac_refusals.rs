//! F.2e reverse enrollment, the refusals (spec §22.13; design
//! `phase-f2e-design.md` §8 RE rows; final review VER-B4): each check that
//! keeps a stranger from becoming a full-authority Mac fails a test when
//! removed. Synthetic data only.

#[path = "../../vault-helper/tests/vault_fx/mod.rs"]
mod vault_fx;
mod mfx;
mod phone_fx;

use std::ffi::{c_char, c_void, CString};
use std::sync::atomic::{AtomicBool, Ordering};

use mfx::*;
use phone_fx::*;
use serde_json::{json, Value};
use vault_ffi::*;
use vault_helper::registry::device::DeviceIdentity;

/// Switches for this file's iPhone callbacks.
static WRONG_MP: AtomicBool = AtomicBool::new(false);
static DECLINE: AtomicBool = AtomicBool::new(false);

extern "C" fn entry_sw(_: *mut c_void, _k: u8, _: u64, a: *mut u8, a_len: *mut usize, _: *mut u8, _: *mut usize, _: usize) -> i32 {
    let mp: &[u8] = if WRONG_MP.load(Ordering::SeqCst) { b"not-the-master-password" } else { MP };
    unsafe {
        std::ptr::copy_nonoverlapping(mp.as_ptr(), a, mp.len());
        *a_len = mp.len();
    }
    SUBMITTED
}
extern "C" fn presence_sw(_: *mut c_void, _: *const c_char) -> bool {
    !DECLINE.load(Ordering::SeqCst)
}

/// The phone with switchable password entry and presence.
fn open_sw(cloud: &Ctx, dir: &std::path::Path) -> *const Ov0Engine {
    let table = Ov0Callbacks {
        ctx: cloud as *const Ctx as *mut c_void,
        secure_entry: Some(entry_sw),
        recovery_sheet: Some(sheet),
        presence: Some(presence_sw),
        capture_suppressed: Some(yes),
        event: Some(event),
        http_send: Some(http),
    };
    let path = CString::new(dir.to_str().unwrap()).unwrap();
    let e = unsafe { ov0_engine_open(path.as_ptr(), &table) };
    assert!(!e.is_null());
    assert_eq!(call(e, r#"{"op":"begin_recovery_unlock","kind":"mp"}"#)["ok"], true);
    e
}

fn qr(begun: &Value) -> Value {
    json!({ "v": 3, "host": "192.168.1.20", "port": 4500, "fp": "cd".repeat(32), "secret": begun["secret"], "commit": begun["commit"] })
}

/// Paired with Mac `a`, revoked it; `publish` decides whether that landed.
fn phone(tag: &str, publish: bool) -> (Box<Ctx>, Mac, Mac, *const Ov0Engine) {
    WRONG_MP.store(false, Ordering::SeqCst);
    DECLINE.store(false, Ordering::SeqCst);
    let (cloud, a, b) = pair(tag);
    let e = open_sw(&cloud, &b.dir);
    assert_eq!(call(e, r#"{"op":"provider_sync"}"#)["ok"], true);
    let mac = vault_helper::crypto::hex::encode(a.dev.device_id());
    assert_eq!(call(e, &format!(r#"{{"op":"revoke_device","device_id":"{mac}"}}"#))["ok"], true);
    if publish {
        assert_eq!(call(e, r#"{"op":"provider_publish"}"#)["result"]["committed"], true);
        assert_eq!(call(e, r#"{"op":"provider_sync"}"#)["ok"], true, "fresh");
    }
    (cloud, a, b, e)
}

/// Begin on the Mac, hello and reply through the phone; the reply as the
/// Mac sent it.
fn up_to_reply(mac: &vault_fx::Fx, e: *const Ov0Engine) -> (Value, Value) {
    let begun = mac.op(json!({"op": "renroll_begin", "fp": "cd".repeat(32), "name": "Synthetic New Mac"}));
    let start = call(e, &json!({"op": "authorize_begin", "qr": qr(&begun)}).to_string());
    assert_eq!(start["ok"], true, "{start}");
    let reply = mac.op(json!({"op": "renroll_hello", "hello": start["hello"]}));
    assert_eq!(reply["ok"], true, "{reply}");
    (start, reply)
}

/// RE-02: a reply that does not open the QR's commitment — another key or
/// another name — is refused; no code is shown.
#[test]
fn re02_a_reply_must_open_the_commitment() {
    let _g = serial();
    let mac = vault_fx::fx();
    let (_cloud, _a, _b, e) = phone("re-02", true);
    let (_, reply) = up_to_reply(&mac, e);
    let mut swapped = reply["reply"].clone();
    swapped["sign_pub"] = swapped["agree_pub"].clone();
    assert_eq!(call(e, &json!({"op": "authorize_reply", "reply": swapped}).to_string())["ok"], false, "a swapped key");
    let mut renamed = reply["reply"].clone();
    renamed["name"] = json!("Someone Else's Mac");
    assert_eq!(call(e, &json!({"op": "authorize_reply", "reply": renamed}).to_string())["error"], "PROTOCOL_VIOLATION", "another name");
    unsafe { ov0_engine_close(e) };
    mac.remove_dir();
}

/// RE-05: five wrong secrets end the attempt; a second correct hello (two
/// devices saw the QR) ends it too.
#[test]
fn re05_the_secret_is_single_use_and_limited() {
    let _g = serial();
    let mac = vault_fx::fx();
    let (_cloud, _a, _b, e) = phone("re-05", true);
    let begun = mac.op(json!({"op": "renroll_begin", "fp": "cd".repeat(32)}));
    let start = call(e, &json!({"op": "authorize_begin", "qr": qr(&begun)}).to_string());
    let mut wrong = start["hello"].clone();
    wrong["secret"] = json!(vault_helper::enroll::transcript::encode_secret(&[0x42; 16]));
    for _ in 0..5 {
        assert_eq!(mac.op(json!({"op": "renroll_hello", "hello": wrong}))["error"], "WRONG_CREDENTIAL");
    }
    assert!(!mac.dir.join("renroll").exists(), "five failures end the attempt and its staging");
    assert_eq!(mac.op(json!({"op": "renroll_hello", "hello": start["hello"]}))["ok"], false, "even the right secret, after");
    // A fresh attempt: the right hello once, then again.
    let begun = mac.op(json!({"op": "renroll_begin", "fp": "cd".repeat(32)}));
    let start = call(e, &json!({"op": "authorize_begin", "qr": qr(&begun)}).to_string());
    assert_eq!(mac.op(json!({"op": "renroll_hello", "hello": start["hello"]}))["ok"], true);
    assert_eq!(mac.op(json!({"op": "renroll_hello", "hello": start["hello"]}))["error"], "PROTOCOL_VIOLATION");
    assert!(!mac.dir.join("renroll").exists(), "a second correct hello ends it");
    unsafe { ov0_engine_close(e) };
    mac.remove_dir();
}

/// RE-06 (the rest): a Mac revocation not yet published (a pending
/// rotation), and a phone that
/// has not just heard from the provider, are refused before any prompt.
#[test]
fn re06_pending_revocation_and_not_fresh() {
    let _g = serial();
    let mac = vault_fx::fx();
    let (cloud, _a, b, e) = phone("re-06b", false);
    let begun = mac.op(json!({"op": "renroll_begin", "fp": "cd".repeat(32)}));
    let frame = json!({"op": "authorize_begin", "qr": qr(&begun)}).to_string();
    // An unpublished revocation is a pending rotation: refused as such
    // (the "pending rotation" and "pending Mac revocation" rows at once).
    assert_eq!(call(e, &frame)["error"], "ROTATION_PENDING", "the revocation has not landed");
    assert_eq!(call(e, r#"{"op":"provider_publish"}"#)["result"]["committed"], true);
    // Reopened, the phone has not checked with the provider yet.
    unsafe { ov0_engine_close(e) };
    let e = open_sw(&cloud, &b.dir);
    assert_eq!(call(e, &frame)["error"], "NOT_FRESH");
    assert_eq!(call(e, r#"{"op":"provider_sync"}"#)["ok"], true);
    assert_eq!(call(e, &frame)["ok"], true, "fresh, and a replacement");
    unsafe { ov0_engine_close(e) };
    mac.remove_dir();
}

/// RE-07: on the iPhone, a declined Face ID or a wrong master password
/// signs nothing and builds no bundle; the wrong password backs off.
#[test]
fn re07_presence_and_master_password_on_the_iphone() {
    let _g = serial();
    let mac = vault_fx::fx();
    let (_cloud, _a, _b, e) = phone("re-07", true);
    let (_, reply) = up_to_reply(&mac, e);
    assert_eq!(call(e, &json!({"op": "authorize_reply", "reply": reply["reply"]}).to_string())["ok"], true);
    DECLINE.store(true, Ordering::SeqCst);
    let declined = call(e, r#"{"op":"authorize_confirm"}"#);
    assert_eq!(declined["error"], "PRESENCE_DENIED", "{declined}");
    assert!(declined.get("session").is_none(), "no bundle");
    DECLINE.store(false, Ordering::SeqCst);
    WRONG_MP.store(true, Ordering::SeqCst);
    let wrong = call(e, r#"{"op":"authorize_confirm"}"#);
    assert_eq!(wrong["error"], "WRONG_CREDENTIAL", "{wrong}");
    assert!(wrong.get("session").is_none());
    let again = call(e, r#"{"op":"authorize_confirm"}"#);
    assert_ne!(again["ok"], true, "backed off: {again}");
    let devices = call(e, r#"{"op":"list_devices"}"#);
    assert!(!devices.to_string().contains("Synthetic New Mac"), "no entry for the new Mac: {devices}");
    WRONG_MP.store(false, Ordering::SeqCst);
    unsafe { ov0_engine_close(e) };
    mac.remove_dir();
}

/// Read the phone's bundle, let `mutate` change it, stream it into the Mac.
fn carry_tampered(e: *const Ov0Engine, mac: &vault_fx::Fx, confirmed: &Value, mutate: &dyn Fn(&mut Value)) -> String {
    let (session, sha, size) = (confirmed["session"].as_str().unwrap(), confirmed["stream"].as_str().unwrap(), confirmed["size"].as_u64().unwrap());
    let mut bytes = Vec::new();
    while (bytes.len() as u64) < size {
        let r = call(e, &json!({"op": "stream_read", "session": session, "sha256": sha, "offset": bytes.len()}).to_string());
        bytes.extend(vault_proto::b64::decode(r["data"].as_str().unwrap()).unwrap());
    }
    let mut bundle: Value = serde_json::from_slice(&bytes).unwrap();
    mutate(&mut bundle);
    let bytes = serde_json::to_vec(&bundle).unwrap();
    let sha = vault_helper::crypto::hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&bytes));
    let s = mac.op(json!({"op": "renroll_bundle_begin", "sha256": sha, "size": bytes.len()}));
    let mac_session = s["session"].as_str().expect("bundle session").to_string();
    let st = mac.op(json!({"op": "stream_begin", "session": mac_session, "sha256": sha, "size": bytes.len()}));
    let stream = st["stream_id"].as_str().unwrap().to_string();
    for (seq, chunk) in bytes.chunks(48 * 1024).enumerate() {
        assert_eq!(mac.op(json!({"op": "stream_write", "session": mac_session, "stream_id": stream, "seq": seq, "offset": seq * 48 * 1024, "data": vault_proto::b64::encode(chunk)}))["ok"], true);
    }
    assert_eq!(mac.op(json!({"op": "stream_end", "session": mac_session, "stream_id": stream}))["ok"], true);
    mac_session
}

/// Flip the last hex digit of a field.
fn flip(v: &mut Value, key: &str) {
    let mut s = v[key].as_str().unwrap().to_string();
    let last = s.pop().unwrap();
    s.push(if last == '0' { '1' } else { '0' });
    v[key] = json!(s);
}

/// RE-04: bundle negatives — another vault, a broken manifest signature, a
/// registry head that is not the one the anchors bind, a checkpoint that
/// does not open under the vault key. Each is refused at
/// `renroll_complete`; nothing is kept and no ACK is signed.
#[test]
fn re04_a_tampered_bundle_is_refused_and_leaves_nothing() {
    let _g = serial();
    let mac = vault_fx::fx();
    let (_cloud, _a, _b, e) = phone("re-04", true);
    let cases: [(&str, &dyn Fn(&mut Value)); 4] = [
        ("another vault", &|v: &mut Value| v["vault_id"] = json!("77".repeat(16))),
        ("manifest signature", &|v: &mut Value| flip(v, "manifest")),
        ("registry head", &|v: &mut Value| flip(v, "registry_head")),
        ("checkpoint", &|v: &mut Value| flip(v, "checkpoint")),
    ];
    for (what, mutate) in cases {
        let (_, reply) = up_to_reply(&mac, e);
        assert_eq!(call(e, &json!({"op": "authorize_reply", "reply": reply["reply"]}).to_string())["ok"], true, "{what}");
        mac.push_panel(vault_fx::submitted(MP));
        assert_eq!(mac.op(json!({"op": "renroll_confirm"}))["ok"], true, "{what}");
        let confirmed = call(e, r#"{"op":"authorize_confirm"}"#);
        assert_eq!(confirmed["ok"], true, "{what}: {confirmed}");
        let session = carry_tampered(e, &mac, &confirmed, mutate);
        let r = mac.op(json!({"op": "renroll_complete", "session": session}));
        assert_ne!(r["ok"], true, "{what}: {r}");
        assert!(r.get("ack").is_none(), "{what}: no ACK");
        assert!(!mac.dir.join("renroll").exists() && !mac.dir.join("vault.db").exists(), "{what}: nothing kept");
        let _ = call(e, r#"{"op":"authorize_abort"}"#);
    }
    unsafe { ov0_engine_close(e) };
    mac.remove_dir();
}
