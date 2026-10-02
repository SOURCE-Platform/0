//! FFI-01 (spec v0.5 §22.2): the C ABI is exactly the catalogue, its op
//! allowlist holds nothing Mac-only or authority-granting beyond the
//! catalogue, and a handle behaves: unknown and Mac-only ops answer
//! `UNKNOWN_OP`, `get_state` answers at once, answers are freed zeroed.
//! Synthetic data only.

use std::ffi::{c_char, c_void, CString};
use std::path::Path;

use vault_ffi::*;

/// Every `#[no_mangle]` function in this crate's sources.
fn exported_in_source() -> Vec<String> {
    let mut out = Vec::new();
    for f in std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("src")).unwrap().flatten() {
        let text = std::fs::read_to_string(f.path()).unwrap();
        let mut lines = text.lines();
        while let Some(l) = lines.next() {
            if l.trim() == "#[no_mangle]" {
                let sig = lines.next().unwrap();
                let name = sig.split("fn ").nth(1).unwrap().split('(').next().unwrap();
                out.push(name.to_string());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn the_exported_symbols_are_exactly_the_catalogue() {
    let mut cat: Vec<String> = CATALOGUE.iter().map(|s| s.to_string()).collect();
    cat.sort();
    assert_eq!(exported_in_source(), cat);
    // No other C-ABI export anywhere in the crate: `extern "C" fn` items
    // appear only as the five entry points and the callback table's types.
    let lib = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs")).unwrap();
    assert_eq!(lib.matches("pub unsafe extern \"C\" fn").count(), CATALOGUE.len());
}

/// The built static library defines no `ov0_` symbol beyond the catalogue
/// and the §2.12 bridge it carries on macOS.
#[test]
fn the_built_library_defines_nothing_else() {
    let lib = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug/libvault_ffi.a");
    let Ok(out) = std::process::Command::new("nm").args(["-gU", "-j"]).arg(&lib).output() else { return };
    let text = String::from_utf8_lossy(&out.stdout);
    for sym in text.lines().filter(|s| s.starts_with("_ov0_")) {
        let name = &sym[1..];
        let bridge = name.starts_with("ov0_se_") || name.starts_with("ov0_hpke_");
        assert!(bridge || CATALOGUE.contains(&name), "unexpected export {name}");
    }
}

#[test]
fn the_op_allowlist_holds_nothing_mac_only() {
    for op in [
        "setup_vault", "peer_serve", "peer_serve_begin", "begin_enrollment", "enroll_hello", "enroll_confirm", "enroll_ack",
        "cancel_enrollment", "revoke_device", "recovery_begin", "recovery_preview", "recovery_complete", "setup_retry_handle",
    ] {
        assert!(!IOS_OPS.contains(&op), "{op} is not an F.2b phone op");
    }
}

extern "C" fn entry(_: *mut c_void, _: u8, _: u64, _: *mut u8, _: *mut usize, _: *mut u8, _: *mut usize, _: usize) -> i32 {
    1 // cancelled
}
extern "C" fn sheet(_: *mut c_void, _: *const u8, _: usize, _: *const c_char, _: *const c_char, _: *const c_char, _: u64) -> i32 {
    1
}
extern "C" fn presence(_: *mut c_void, _: *const c_char) -> bool {
    false
}
extern "C" fn capture(_: *mut c_void, _: *const c_char) -> bool {
    false
}
extern "C" fn event(_: *mut c_void, _: *const u8, _: usize) {}

fn call(e: *const Ov0Engine, req: &str) -> serde_json::Value {
    let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
    // SAFETY: a live handle; buffers as the catalogue says.
    assert_eq!(unsafe { ov0_engine_call(e, req.as_ptr(), req.len(), &mut out, &mut len) }, 0);
    let v = serde_json::from_slice(unsafe { std::slice::from_raw_parts(out, len) }).unwrap();
    unsafe { ov0_engine_free(out, len) };
    v
}

#[test]
fn a_handle_answers_only_the_allowlist() {
    let dir = std::env::temp_dir().join(format!("vffi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cb = Ov0Callbacks { ctx: std::ptr::null_mut(), secure_entry: entry, recovery_sheet: sheet, presence, capture_suppressed: capture, event };
    let path = CString::new(dir.to_str().unwrap()).unwrap();
    // SAFETY: valid path and table, both outliving the handle.
    let e = unsafe { ov0_engine_open(path.as_ptr(), &cb) };
    assert!(!e.is_null());
    assert_eq!(call(e, r#"{"op":"get_state"}"#)["state"], "uninitialized");
    for op in ["setup_vault", "peer_serve", "enroll_confirm", "no_such_op"] {
        assert_eq!(call(e, &format!(r#"{{"op":"{op}"}}"#))["error"], "UNKNOWN_OP", "{op}");
    }
    assert_eq!(call(e, "not json")["error"], "INVALID_INPUT");
    // An allowed op still meets the engine's own state rules.
    assert_eq!(call(e, r#"{"op":"list_items"}"#)["ok"], false);
    unsafe { ov0_engine_lock(e) };
    unsafe { ov0_engine_close(e) };
    let _ = std::fs::remove_dir_all(&dir);
}
