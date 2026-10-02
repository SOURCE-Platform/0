//! SOURCE Vault's C ABI onto the vault engine (spec v0.5 §22.2; catalogue
//! in docs/security/phase-f2b-ffi.md). Five entry points and nothing else
//! (`CATALOGUE`, enforced by `tests/catalogue.rs`):
//!
//! - `ov0_engine_open(vault_dir, callbacks)` → engine handle (or null);
//! - `ov0_engine_call(engine, request, len, &out, &out_len)` → one §1.5
//!   op as JSON, limited to `IOS_OPS`; the answer is a JSON buffer;
//! - `ov0_engine_lock(engine)` — preempts whatever op is waiting;
//! - `ov0_engine_free(buf, len)` — zeroes, then frees, an answer buffer;
//! - `ov0_engine_close(engine)` — locks, then drops the handle.
//!
//! Never across: PK, `RK_bytes`, `sk_c`, `ikm_c`, a signature over a
//! caller-supplied digest, raw key bytes. The audited crossings are (a) the
//! VK as the return of the Secure Enclave envelope open (the `ov0_hpke_*`
//! bridge, linked from `Bridge.swift`), (b) MP / RK words in and (c) RK
//! words out through the callbacks of `callbacks.rs`, (d) one record's
//! plaintext per reveal/edit answer or add/update request, (e) list
//! metadata. No unwinding crosses: a panic aborts the process.

mod callbacks;

use std::ffi::{c_char, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use vault_engine::errors::ErrorCode;
use vault_engine::vault::{dispatch, lock_core, LockReason, OpOutcome, VaultCore};
use zeroize::Zeroize;

pub use callbacks::Ov0Callbacks;

/// Every exported symbol, in order. Adding one is a spec change (§22.2).
pub const CATALOGUE: &[&str] = &["ov0_engine_open", "ov0_engine_call", "ov0_engine_lock", "ov0_engine_free", "ov0_engine_close"];

/// The §1.5 ops SOURCE Vault may run (F.2b). Everything else answers
/// `UNKNOWN_OP` — in particular the Mac-only serving, enrollment
/// authorization, vault creation and total-loss recovery ops.
pub const IOS_OPS: &[&str] = &[
    "get_state", "unlock", "begin_recovery_unlock", "list_items", "reveal", "add_item", "update_item", "delete_item",
    "list_history", "list_deleted", "restore_revision", "resolve_conflict", "change_master_password", "rotate_recovery_key",
    "list_devices", "registry_status", "set_auto_lock_minutes", "backup_prepare", "backup_blob_list", "backup_transition_body",
    "backup_commit_result", "backup_state_offer", "backup_apply", "stream_read", "stream_begin", "stream_write", "stream_end",
    "stream_cancel", "sign_provider_request", "session_close", "quarantine_status", "remote_update_status",
];

pub struct Ov0Engine {
    core: Arc<Mutex<VaultCore>>,
    deps: vault_engine::vault::Deps,
    /// One op at a time, as the helper's executor runs them; `lock` does
    /// not take it, so it preempts an op waiting on the user.
    lane: Mutex<()>,
}

fn guarded<T>(f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| std::process::abort())
}

/// # Safety
/// `vault_dir` is a NUL-terminated UTF-8 path; `callbacks` points to a
/// table that outlives the engine.
#[no_mangle]
pub unsafe extern "C" fn ov0_engine_open(vault_dir: *const c_char, callbacks: *const Ov0Callbacks) -> *mut Ov0Engine {
    guarded(|| {
        if vault_dir.is_null() || callbacks.is_null() {
            return std::ptr::null_mut();
        }
        let Ok(dir) = CStr::from_ptr(vault_dir).to_str() else { return std::ptr::null_mut() };
        let cb = callbacks::Shared::new(*callbacks);
        let engine = Ov0Engine { core: Arc::new(Mutex::new(VaultCore::boot(PathBuf::from(dir)))), deps: cb.deps(), lane: Mutex::new(()) };
        Box::into_raw(Box::new(engine))
    })
}

/// One op. Returns 0 with a JSON answer in `*out` (free it with
/// `ov0_engine_free`), or -1 for unusable arguments.
///
/// # Safety
/// `engine` came from `ov0_engine_open`; `request` holds `len` bytes; `out`
/// and `out_len` are writable.
#[no_mangle]
pub unsafe extern "C" fn ov0_engine_call(engine: *const Ov0Engine, request: *const u8, len: usize, out: *mut *mut u8, out_len: *mut usize) -> i32 {
    guarded(|| {
        if engine.is_null() || request.is_null() || out.is_null() || out_len.is_null() {
            return -1;
        }
        let e = &*engine;
        // A request may carry one record's fields (crossing d): our copy is
        // zeroed as soon as it is parsed.
        let mut bytes = std::slice::from_raw_parts(request, len).to_vec();
        let frame: Option<Value> = serde_json::from_slice(&bytes).ok();
        bytes.zeroize();
        let answer = match frame {
            Some(frame) => run(e, &frame).response,
            None => OpOutcome::err(ErrorCode::InvalidInput).response,
        };
        let mut buf = serde_json::to_vec(&answer).unwrap_or_default().into_boxed_slice();
        *out_len = buf.len();
        *out = buf.as_mut_ptr();
        std::mem::forget(buf);
        0
    })
}

fn run(e: &Ov0Engine, frame: &Value) -> OpOutcome {
    let op = frame.get("op").and_then(Value::as_str).unwrap_or("");
    if !IOS_OPS.contains(&op) {
        return OpOutcome::err(ErrorCode::UnknownOp);
    }
    if op == "get_state" {
        // Answered at once, like the helper's server layer: never queued
        // behind an op that waits on the user.
        let c = lock_core(&e.core);
        return OpOutcome::ok(json!({ "state": c.reported_state().as_str() }));
    }
    let _lane = e.lane.lock().unwrap_or_else(|p| p.into_inner());
    dispatch::dispatch(&e.core, frame, &e.deps)
}

/// # Safety
/// `engine` came from `ov0_engine_open`.
#[no_mangle]
pub unsafe extern "C" fn ov0_engine_lock(engine: *const Ov0Engine) {
    guarded(|| {
        if let Some(e) = engine.as_ref() {
            let events = lock_core(&e.core).lock(LockReason::Explicit);
            for ev in events {
                e.deps.events.emit(ev);
            }
        }
    })
}

/// # Safety
/// `buf`/`len` are exactly an answer from `ov0_engine_call`, freed once.
#[no_mangle]
pub unsafe extern "C" fn ov0_engine_free(buf: *mut u8, len: usize) {
    guarded(|| {
        if !buf.is_null() {
            let mut b = Box::from_raw(std::ptr::slice_from_raw_parts_mut(buf, len));
            b.zeroize();
        }
    })
}

/// # Safety
/// `engine` came from `ov0_engine_open` and is not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn ov0_engine_close(engine: *mut Ov0Engine) {
    guarded(|| {
        if !engine.is_null() {
            let e = Box::from_raw(engine);
            let _ = lock_core(&e.core).lock(LockReason::Explicit);
        }
    })
}
