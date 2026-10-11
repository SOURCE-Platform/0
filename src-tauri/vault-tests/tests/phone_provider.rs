//! F.2d steps 2–4 through SOURCE Vault's C ABI: a second device's vault,
//! opened with `ov0_engine_open` like the iPhone app does, syncs with the
//! provider core and publishes its own edit through `provider_sync` /
//! `provider_publish`; the provider HTTPS is the `http_send` callback.
//! A provider-confirmed state lifts a removal marker (§22.9). Synthetic
//! data only.

mod mfx;

use std::ffi::{c_char, c_void, CStr, CString};

use mfx::*;
use serde_json::Value;
use vault_ffi::*;

fn s(p: *const c_char) -> String {
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

extern "C" fn http(ctx: *mut c_void, _origin: *const c_char, method: *const c_char, path: *const c_char, auth: *const c_char, body: *const u8, body_len: usize, _t: u64, out: *mut u8, cap: usize, out_len: *mut usize, status: *mut u16, date: *mut u64) -> i32 {
    let cloud = unsafe { &*(ctx as *const Cloud) };
    let auth = (!auth.is_null()).then(|| s(auth));
    let body = unsafe { std::slice::from_raw_parts(body, body_len) };
    let r = cloud.send(&s(method), &s(path), auth.as_deref(), body);
    if r.body.len() > cap {
        return 2;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(r.body.as_ptr(), out, r.body.len());
        *out_len = r.body.len();
        *status = r.status;
        *date = now();
    }
    0
}

extern "C" fn entry(_: *mut c_void, _kind: u8, _: u64, a: *mut u8, a_len: *mut usize, _: *mut u8, _: *mut usize, _: usize) -> i32 {
    unsafe {
        std::ptr::copy_nonoverlapping(MP.as_ptr(), a, MP.len());
        *a_len = MP.len();
    }
    SUBMITTED
}
extern "C" fn sheet(_: *mut c_void, _: *const u8, _: usize, _: *const c_char, _: *const c_char, _: *const c_char, _: u64) -> i32 {
    SUBMITTED
}
extern "C" fn yes(_: *mut c_void, _: *const c_char) -> bool {
    true
}
extern "C" fn event(_: *mut c_void, _: *const u8, _: usize) {}

fn call(e: *const Ov0Engine, frame: &str) -> Value {
    let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
    assert_eq!(unsafe { ov0_engine_call(e, frame.as_ptr(), frame.len(), &mut out, &mut len) }, 0);
    let v = serde_json::from_slice(unsafe { std::slice::from_raw_parts(out, len) }).unwrap();
    unsafe { ov0_engine_free(out) };
    v
}

/// A Mac (A) and a second device (B) whose vault runs behind the C ABI.
fn pair(tag: &str) -> (Box<Cloud>, Mac, Mac) {
    let cloud = Box::new(Cloud::new(tag));
    let mut a = Mac::new(&format!("{tag}-a"));
    a.setup(&cloud, &format!("synthetic-{tag}@example.test")).unwrap();
    let mut b = Mac::new(&format!("{tag}-b"));
    a.enroll(&cloud, &b);
    b.join(&cloud, a.vid());
    // The simulator keeps B's identity beside its vault directory; the
    // engine (like the iPhone app) finds it inside.
    std::fs::copy(b.root.join("device.json"), b.dir.join("device.json")).unwrap();
    // The pairing signs the vault key's commitment (§22.4); the
    // simulator's join does not, so do it here as the bundle path would.
    let gen = b.store().header.vk_generation;
    vault_helper::vault::vk_commit::record(&b.dir, &b.vid(), gen, b.vk.as_ref().unwrap()).unwrap();
    b.store = None;
    b.vk = None;
    (cloud, a, b)
}

fn open(cloud: &Cloud, dir: &std::path::Path) -> *const Ov0Engine {
    let table = Ov0Callbacks {
        ctx: cloud as *const Cloud as *mut c_void,
        secure_entry: Some(entry),
        recovery_sheet: Some(sheet),
        presence: Some(yes),
        capture_suppressed: Some(yes),
        event: Some(event),
        http_send: Some(http),
    };
    let path = CString::new(dir.to_str().unwrap()).unwrap();
    let e = unsafe { ov0_engine_open(path.as_ptr(), &table) };
    assert!(!e.is_null());
    let u = call(e, r#"{"op":"begin_recovery_unlock","kind":"mp"}"#);
    assert_eq!(u["ok"], true, "{u} / {}", call(e, r#"{"op":"get_state"}"#));
    e
}

#[test]
fn a_phone_engine_syncs_and_publishes_through_the_provider_path() {
    let (cloud, mut a, b) = pair("ffi-provider");
    a.add("from-a");
    a.publish(&cloud).unwrap();
    let e = open(&cloud, &b.dir);
    let synced = call(e, r#"{"op":"provider_sync","interactive":true}"#);
    assert_eq!(synced["ok"], true, "{synced}");
    assert!(call(e, r#"{"op":"list_items"}"#)["items"].to_string().contains("from-a"));
    let add = call(e, r#"{"op":"add_item","kind":"login","title":"from-b","username":"u@example.test","hosts":["example.test"],"password":"synthetic-b"}"#);
    assert_eq!(add["ok"], true, "{add}");
    let published = call(e, r#"{"op":"provider_publish","interactive":true}"#);
    assert_eq!(published["ok"], true, "{published}");
    assert_eq!(published["result"]["committed"], true, "{published}");
    a.sync(&cloud).unwrap();
    assert!(a.titles().contains(&"from-b".to_string()), "the Mac received the phone's edit");
    // The step-wise ops are the flows' alone now (review SEC-O2).
    assert_eq!(call(e, r#"{"op":"backup_prepare"}"#)["error"], "UNKNOWN_OP");
    unsafe { ov0_engine_close(e) };
}

#[test]
fn a_provider_confirmed_state_lifts_the_removal_lock() {
    let (cloud, _a, b) = pair("ffi-lift");
    std::fs::write(b.dir.join("removal.json"), br#"{"published":false,"at":1}"#).unwrap();
    let e = open(&cloud, &b.dir);
    assert!(call(e, r#"{"op":"get_state"}"#).get("removed").is_some());
    let synced = call(e, r#"{"op":"provider_sync"}"#);
    assert_eq!(synced["ok"], true, "{synced}");
    assert!(call(e, r#"{"op":"get_state"}"#).get("removed").is_none(), "lifted: the provider says this device is active");
    unsafe { ov0_engine_close(e) };
}
