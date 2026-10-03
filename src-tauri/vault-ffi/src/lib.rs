//! SOURCE Vault's C ABI onto the vault engine (spec v0.5 §22.2; catalogue
//! in docs/security/phase-f2b-ffi.md). Five entry points and nothing else
//! (`CATALOGUE`, enforced by `tests/catalogue.rs`):
//!
//! - `ov0_engine_open(vault_dir, callbacks)` → engine handle (or null);
//! - `ov0_engine_call(engine, request, len, &out, &out_len)` → one §1.5
//!   op as JSON, limited to `IOS_OPS`; the answer is a JSON buffer;
//! - `ov0_engine_lock(engine)` — locks at once, whatever op is waiting;
//! - `ov0_engine_free(buf)` — zeroes, then frees, an answer buffer;
//! - `ov0_engine_close(engine)` — locks, waits for the op in flight, ends
//!   the auto-lock tick and releases the handle.
//!
//! Never across: PK, `RK_bytes`, `sk_c`, `ikm_c`, a signature over a
//! caller-supplied digest, raw key bytes. The audited crossings are (a) the
//! VK as the plaintext of the Secure Enclave envelope open (the linked
//! §2.12 bridge), (b) MP / RK words in and (c) RK words out through the
//! callbacks of `callbacks.rs`, (d) one record's plaintext per reveal/edit
//! answer or add/update request, (e) list metadata. No unwinding crosses:
//! a panic aborts the process.

mod callbacks;
mod engine;

use std::ffi::{c_char, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::Value;
use vault_engine::errors::ErrorCode;
use vault_engine::vault::{LockReason, OpOutcome};
use zeroize::{Zeroize, Zeroizing};

pub use callbacks::{Ov0Callbacks, Shared, ENTRY_CAP, KIND_MP_CHANGE, KIND_MP_CREATE, KIND_MP_ENTRY, KIND_RK_ENTRY, MIN_MP_CHARS, SUBMITTED};
pub use engine::{Inner as Ov0Engine, IOS_OPS, MAX_REQUEST};

/// Every exported symbol. Adding one is a spec change (§22.2).
pub const CATALOGUE: &[&str] = &["ov0_engine_open", "ov0_engine_call", "ov0_engine_lock", "ov0_engine_free", "ov0_engine_close"];

/// The §2.12 bridge functions the engine calls (supplied by `Bridge.swift`
/// linked into the app) — the complete list (review VER-I6).
pub const BRIDGE_IMPORTS: &[&str] = &[
    "ov0_se_key_create", "ov0_se_key_create_bio", "ov0_se_key_needs_user", "ov0_se_key_public", "ov0_se_key_delete",
    "ov0_se_sign_create", "ov0_se_sign_public", "ov0_se_sign_digest", "ov0_hpke_seal", "ov0_hpke_open_se_auth",
];

/// Bytes in front of every answer: its length, so `ov0_engine_free` never
/// trusts a caller's (review SEC-O1).
const PREFIX: usize = 8;

fn guarded<T>(f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| std::process::abort())
}

/// # Safety
/// `vault_dir` is a NUL-terminated UTF-8 path; `callbacks` points to a
/// complete table whose `ctx` outlives the engine. One handle per
/// directory; open it only while protected data is available.
#[no_mangle]
pub unsafe extern "C" fn ov0_engine_open(vault_dir: *const c_char, callbacks: *const Ov0Callbacks) -> *const Ov0Engine {
    guarded(|| {
        let (Some(dir), Some(cb)) = (vault_dir.as_ref().map(|p| CStr::from_ptr(p)), callbacks.as_ref()) else { return std::ptr::null() };
        let (Ok(dir), Some(shared)) = (dir.to_str(), Shared::new(cb)) else { return std::ptr::null() };
        Arc::into_raw(Ov0Engine::boot(PathBuf::from(dir), shared))
    })
}

/// One op. Returns 0 with a JSON answer in `*out` / `*out_len` (free it
/// with `ov0_engine_free`), or -1 for unusable arguments. Blocks while the
/// op waits on the user: never call it on the main thread, and never from
/// inside a callback.
///
/// # Safety
/// `engine` is a live handle; `request` holds `len` bytes; `out` and
/// `out_len` are writable.
#[no_mangle]
pub unsafe extern "C" fn ov0_engine_call(engine: *const Ov0Engine, request: *const u8, len: usize, out: *mut *mut u8, out_len: *mut usize) -> i32 {
    guarded(|| {
        let Some(e) = engine.as_ref() else { return -1 };
        if request.is_null() || out.is_null() || out_len.is_null() {
            return -1;
        }
        let answer = if len > MAX_REQUEST {
            OpOutcome::err(ErrorCode::InvalidInput).response
        } else {
            // A request may carry one record's fields (crossing d): our
            // copy is zeroed as soon as it is parsed.
            let bytes = Zeroizing::new(std::slice::from_raw_parts(request, len).to_vec());
            match serde_json::from_slice::<Value>(&bytes) {
                Ok(frame) => e.run(&frame).response,
                Err(_) => OpOutcome::err(ErrorCode::InvalidInput).response,
            }
        };
        e.shared.flush();
        let json = Zeroizing::new(serde_json::to_vec(&answer).unwrap_or_default());
        let mut buf = vec![0u8; PREFIX + json.len()].into_boxed_slice();
        buf[..PREFIX].copy_from_slice(&(json.len() as u64).to_le_bytes());
        buf[PREFIX..].copy_from_slice(&json);
        *out_len = json.len();
        *out = Box::into_raw(buf).cast::<u8>().add(PREFIX);
        0
    })
}

/// # Safety
/// `engine` is a live handle.
#[no_mangle]
pub unsafe extern "C" fn ov0_engine_lock(engine: *const Ov0Engine) {
    guarded(|| {
        if let Some(e) = engine.as_ref() {
            e.lock(LockReason::Explicit);
        }
    })
}

/// # Safety
/// `buf` is an answer from `ov0_engine_call`, freed once.
#[no_mangle]
pub unsafe extern "C" fn ov0_engine_free(buf: *mut u8) {
    guarded(|| {
        if !buf.is_null() {
            let start = buf.sub(PREFIX);
            let mut len = [0u8; PREFIX];
            len.copy_from_slice(std::slice::from_raw_parts(start, PREFIX));
            let total = PREFIX + u64::from_le_bytes(len) as usize;
            let mut b = Box::from_raw(std::ptr::slice_from_raw_parts_mut(start, total));
            b.zeroize();
        }
    })
}

/// # Safety
/// `engine` is a live handle, not used again by anyone afterwards.
#[no_mangle]
pub unsafe extern "C" fn ov0_engine_close(engine: *const Ov0Engine) {
    guarded(|| {
        if engine.is_null() {
            return;
        }
        let e = Arc::from_raw(engine);
        e.stop.store(true, Ordering::SeqCst);
        e.lock(LockReason::Explicit);
        // Wait for an op still running (it fails closed at its re-lock
        // checkpoint once Swift dismisses its screen on `locked`).
        drop(e.lane.lock().unwrap_or_else(|p| p.into_inner()));
        drop(e); // the tick holds the other reference and ends within 1 s
    })
}
