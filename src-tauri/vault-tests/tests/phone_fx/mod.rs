//! The SOURCE Vault C ABI harness shared by the phone engine tests: the
//! provider core behind an `http_send` callback, auto-confirming entry,
//! sheet and presence callbacks, and serial execution (the engines share
//! process-wide state). Synthetic data only.

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_void, CStr, CString};

use crate::mfx::*;
use serde_json::Value;
use vault_ffi::*;

pub fn s(p: *const c_char) -> String {
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// The callbacks' context: the provider core, and test switches.
pub struct Ctx {
    pub cloud: Cloud,
    /// No answer at all (`http_send` → 1).
    pub offline: Cell<bool>,
    /// Call back into this engine from inside `http_send` once.
    pub reenter: Cell<*const Ov0Engine>,
    pub reentered: RefCell<Option<Value>>,
    pub origins: RefCell<Vec<String>>,
}

pub extern "C" fn http(ctx: *mut c_void, origin: *const c_char, method: *const c_char, path: *const c_char, auth: *const c_char, body: *const u8, body_len: usize, _t: u64, out: *mut u8, cap: usize, out_len: *mut usize, status: *mut u16, date: *mut u64) -> i32 {
    let ctx = unsafe { &*(ctx as *const Ctx) };
    ctx.origins.borrow_mut().push(s(origin));
    let e = ctx.reenter.replace(std::ptr::null());
    if !e.is_null() {
        *ctx.reentered.borrow_mut() = Some(call(e, r#"{"op":"list_items"}"#));
    }
    if ctx.offline.get() {
        return 1;
    }
    let cloud = &ctx.cloud;
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

pub extern "C" fn entry(_: *mut c_void, _kind: u8, _: u64, a: *mut u8, a_len: *mut usize, _: *mut u8, _: *mut usize, _: usize) -> i32 {
    unsafe {
        std::ptr::copy_nonoverlapping(MP.as_ptr(), a, MP.len());
        *a_len = MP.len();
    }
    SUBMITTED
}
pub extern "C" fn sheet(_: *mut c_void, _: *const u8, _: usize, _: *const c_char, _: *const c_char, _: *const c_char, _: u64) -> i32 {
    SUBMITTED
}
pub extern "C" fn yes(_: *mut c_void, _: *const c_char) -> bool {
    true
}
pub extern "C" fn event(_: *mut c_void, _: *const u8, _: usize) {}

pub fn call(e: *const Ov0Engine, frame: &str) -> Value {
    let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
    assert_eq!(unsafe { ov0_engine_call(e, frame.as_ptr(), frame.len(), &mut out, &mut len) }, 0);
    let v = serde_json::from_slice(unsafe { std::slice::from_raw_parts(out, len) }).unwrap();
    unsafe { ov0_engine_free(out) };
    v
}

/// A Mac (A) and a second device (B) whose vault runs behind the C ABI.
/// One test at a time: the engines share process-wide state (the test
/// Keychain, the floor), and running side by side deadlocked (2026-10-13).
pub fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

pub fn ctx(tag: &str) -> Box<Ctx> {
    Box::new(Ctx { cloud: Cloud::new(tag), offline: Cell::new(false), reenter: Cell::new(std::ptr::null()), reentered: RefCell::new(None), origins: RefCell::new(Vec::new()) })
}

pub fn pair(tag: &str) -> (Box<Ctx>, Mac, Mac) {
    let cloud = ctx(tag);
    let mut a = Mac::new(&format!("{tag}-a"));
    a.setup(&cloud.cloud, &format!("synthetic-{tag}@example.test")).unwrap();
    let mut b = Mac::new(&format!("{tag}-b"));
    a.enroll(&cloud.cloud, &b);
    b.join(&cloud.cloud, a.vid());
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

pub fn open(cloud: &Ctx, dir: &std::path::Path) -> *const Ov0Engine {
    let e = open_raw(cloud, dir);
    let u = call(e, r#"{"op":"begin_recovery_unlock","kind":"mp"}"#);
    assert_eq!(u["ok"], true, "{u} / {}", call(e, r#"{"op":"get_state"}"#));
    e
}

pub fn open_raw(cloud: &Ctx, dir: &std::path::Path) -> *const Ov0Engine {
    let table = Ov0Callbacks {
        ctx: cloud as *const Ctx as *mut c_void,
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
    e
}

