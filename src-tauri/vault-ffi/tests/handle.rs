//! A handle's behaviour (spec v0.5 §22.2, §1.6; phase-f2b-ffi.md): ops
//! outside the allowlist answer `UNKNOWN_OP`; `get_state` and lock never
//! wait behind an op; the callbacks map kinds and enforce the new-MP rule;
//! events wait until the engine is free; the vault locks itself when the
//! auto-lock window runs out. Synthetic data only; Keychain items and SE
//! tags in the run's test namespace.

use std::ffi::{c_char, c_void, CString};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use vault_engine::vault::{PanelOutcome, PanelRequest};
use vault_ffi::*;

const MP: &[u8] = b"synthetic-master-password-0001 (test fixture, not real)";

static SERIAL: Mutex<()> = Mutex::new(());
/// What the fake secure entry answers next: (a, b, result).
static NEXT: Mutex<(Vec<u8>, Vec<u8>, i32)> = Mutex::new((Vec::new(), Vec::new(), 0));
static KINDS: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static SHEETS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn entry(_: *mut c_void, kind: u8, _: u64, a: *mut u8, a_len: *mut usize, b: *mut u8, b_len: *mut usize, cap: usize) -> i32 {
    KINDS.lock().unwrap().push(kind);
    let (x, y, rc) = NEXT.lock().unwrap().clone();
    assert!(x.len() <= cap && y.len() <= cap);
    // SAFETY: engine-owned buffers of `cap` bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(x.as_ptr(), a, x.len());
        std::ptr::copy_nonoverlapping(y.as_ptr(), b, y.len());
        *a_len = x.len();
        *b_len = y.len();
    }
    rc
}
extern "C" fn sheet(_: *mut c_void, _: *const u8, len: usize, _: *const c_char, _: *const c_char, _: *const c_char, _: u64) -> i32 {
    assert!(len > 0);
    SHEETS.fetch_add(1, Ordering::SeqCst);
    0
}
extern "C" fn yes(_: *mut c_void, _: *const c_char) -> bool {
    true
}
extern "C" fn event(_: *mut c_void, json: *const u8, len: usize) {
    // SAFETY: `len` bytes of JSON, valid for the call.
    EVENTS.lock().unwrap().push(String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(json, len) }).into_owned());
}

fn table() -> Ov0Callbacks {
    Ov0Callbacks { ctx: std::ptr::null_mut(), secure_entry: Some(entry), recovery_sheet: Some(sheet), presence: Some(yes), capture_suppressed: Some(yes), event: Some(event) }
}

fn open(tag: &str) -> (*const Ov0Engine, std::path::PathBuf) {
    vault_engine::test_support::init_test_namespace();
    let dir = std::env::temp_dir().join(format!("vffi-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = CString::new(dir.to_str().unwrap()).unwrap();
    let cb = Box::leak(Box::new(table()));
    // SAFETY: a valid path and a table that lives for the process.
    let e = unsafe { ov0_engine_open(path.as_ptr(), cb) };
    assert!(!e.is_null());
    (e, dir)
}

fn call(e: *const Ov0Engine, req: &str) -> serde_json::Value {
    let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
    // SAFETY: a live handle; buffers as the catalogue says.
    assert_eq!(unsafe { ov0_engine_call(e, req.as_ptr(), req.len(), &mut out, &mut len) }, 0);
    let v = serde_json::from_slice(unsafe { std::slice::from_raw_parts(out, len) }).unwrap();
    unsafe { ov0_engine_free(out) };
    v
}

#[test]
fn a_handle_answers_only_the_allowlist() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let (e, dir) = open("allow");
    assert_eq!(call(e, r#"{"op":"get_state"}"#)["state"], "uninitialized");
    for op in ["setup_vault", "peer_serve", "peer_serve_begin", "begin_enrollment", "enroll_hello", "enroll_confirm", "enroll_ack", "cancel_enrollment", "revoke_device", "recovery_begin", "recovery_preview", "recovery_complete", "setup_retry_handle", "rotate_recovery_key", "no_such_op"] {
        assert_eq!(call(e, &format!(r#"{{"op":"{op}"}}"#))["error"], "UNKNOWN_OP", "{op}");
    }
    assert_eq!(call(e, "not json")["error"], "INVALID_INPUT");
    let big = format!(r#"{{"op":"list_items","pad":"{}"}}"#, "x".repeat(MAX_REQUEST));
    assert_eq!(call(e, &big)["error"], "INVALID_INPUT", "over the frame cap");
    let missing = Ov0Callbacks { event: None, ..table() };
    let path = CString::new(dir.to_str().unwrap()).unwrap();
    assert!(unsafe { ov0_engine_open(path.as_ptr(), &missing) }.is_null(), "an incomplete table is refused");
    unsafe { ov0_engine_close(e) };
    let _ = std::fs::remove_dir_all(&dir);
}

/// VER-I2: `get_state` and lock answer while an op holds the lane.
#[test]
fn state_and_lock_never_wait_behind_an_op() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let (e, dir) = open("lane");
    // SAFETY: the handle outlives this scope.
    let lane = unsafe { &(*e).lane }.lock().unwrap();
    let addr = e as usize;
    let t = std::thread::spawn(move || {
        let e = addr as *const Ov0Engine;
        let started = Instant::now();
        let s = call(e, r#"{"op":"get_state"}"#);
        unsafe { ov0_engine_lock(e) };
        (s, started.elapsed())
    });
    let (s, took) = t.join().unwrap();
    drop(lane);
    assert_eq!(s["ok"], true);
    assert!(took < Duration::from_secs(1), "{took:?}");
    unsafe { ov0_engine_close(e) };
    let _ = std::fs::remove_dir_all(&dir);
}

/// VER-I3 / SEC-I2: kinds, the MP-change pair, cancels, and the new-MP
/// rule (at least 8 characters of UTF-8, as the Mac panel requires).
#[test]
fn the_secure_entry_callback_maps_kinds_and_refuses_weak_new_mps() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let shared = Shared::new(&table()).unwrap();
    let deps = shared.deps();
    let t = Duration::from_secs(1);
    *NEXT.lock().unwrap() = (b"old-password-1".to_vec(), b"new-password-2".to_vec(), SUBMITTED);
    KINDS.lock().unwrap().clear();
    assert!(matches!(deps.panel.run(PanelRequest::MpChange, t), PanelOutcome::SubmittedChange(a, b) if a.as_slice() == b"old-password-1" && b.as_slice() == b"new-password-2"));
    assert!(matches!(deps.panel.run(PanelRequest::MpEntry, t), PanelOutcome::Submitted(a) if a.as_slice() == b"old-password-1"));
    assert!(matches!(deps.panel.run(PanelRequest::RkEntry, t), PanelOutcome::Submitted(_)));
    assert!(matches!(deps.panel.run(PanelRequest::MpCreate, t), PanelOutcome::Submitted(_)));
    assert_eq!(*KINDS.lock().unwrap(), vec![KIND_MP_CHANGE, KIND_MP_ENTRY, KIND_RK_ENTRY, KIND_MP_CREATE]);
    *NEXT.lock().unwrap() = (b"old-password-1".to_vec(), b"short".to_vec(), SUBMITTED);
    assert!(matches!(deps.panel.run(PanelRequest::MpChange, t), PanelOutcome::Cancelled), "a new MP under 8 characters");
    *NEXT.lock().unwrap() = (vec![0xFF; 12], Vec::new(), SUBMITTED);
    assert!(matches!(deps.panel.run(PanelRequest::MpCreate, t), PanelOutcome::Cancelled), "not UTF-8");
    *NEXT.lock().unwrap() = (b"whatever-it-is".to_vec(), Vec::new(), 1);
    assert!(matches!(deps.panel.run(PanelRequest::MpEntry, t), PanelOutcome::Cancelled), "the user cancelled");
}

/// SEC-I4: events reach Swift only at `flush`, in order.
#[test]
fn events_wait_for_flush() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let shared = Shared::new(&table()).unwrap();
    let deps = shared.deps();
    EVENTS.lock().unwrap().clear();
    deps.events.emit(serde_json::json!({"event": "one"}));
    deps.events.emit(serde_json::json!({"event": "two"}));
    assert!(EVENTS.lock().unwrap().is_empty());
    shared.flush();
    assert_eq!(*EVENTS.lock().unwrap(), vec![r#"{"event":"one"}"#, r#"{"event":"two"}"#]);
}

/// SEC-B1 / VER-I4: an unlocked vault locks itself when its window runs
/// out, with no call from Swift. (The vault is made through the engine's
/// own dispatch, as the Mac would; `setup_vault` is not a phone op.)
#[test]
fn the_vault_locks_itself_when_the_window_runs_out() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let (e, dir) = open("autolock");
    *NEXT.lock().unwrap() = (MP.to_vec(), Vec::new(), SUBMITTED);
    // SAFETY: the handle is live.
    let inner = unsafe { &*e };
    let setup = vault_engine::vault::dispatch::dispatch(&inner.core, &serde_json::json!({"op": "setup_vault", "handle": "synthetic-ffi@example.test"}), &inner.deps);
    assert_eq!(setup.response["ok"], true, "{}", setup.response);
    assert_eq!(call(e, r#"{"op":"begin_recovery_unlock","kind":"mp"}"#)["ok"], true);
    // A first publication is staged, so the reported state may be
    // BACKING_UP over the vault; `vault_open` says whether the key is here.
    assert_eq!(call(e, r#"{"op":"get_state"}"#)["vault_open"], true);
    std::env::set_var("OV0_VAULT_AUTO_LOCK_SECS", "1");
    std::thread::sleep(Duration::from_millis(2600));
    std::env::remove_var("OV0_VAULT_AUTO_LOCK_SECS");
    assert_eq!(call(e, r#"{"op":"get_state"}"#)["vault_open"], false, "the key is gone");
    assert!(EVENTS.lock().unwrap().iter().any(|e| e.contains("locked")), "Swift was told");
    unsafe { ov0_engine_close(e) };
    vault_engine::device::identity::wipe(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}
