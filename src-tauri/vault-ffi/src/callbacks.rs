//! The Swift callback table (spec v0.5 §22.2): SOURCE Vault's secure entry,
//! Recovery Key sheet, presence check, capture check and event sink, as
//! the engine's `Deps` services. Secrets cross only here and only as the
//! catalogue allows: MP / RK words **in** (written by Swift into
//! engine-owned zeroizing buffers, crossing b) and RK words **out** to the
//! sheet (crossing c).

use std::ffi::{c_char, c_void, CString};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use vault_engine::vault::{
    CaptureChecker, Deps, EventSink, PanelOutcome, PanelRequest, PanelRunner, PresenceChecker, RecoverySheet,
};
use zeroize::Zeroizing;

/// Longest secret a secure-entry field may hand over (an MP or 24 words).
pub const ENTRY_CAP: usize = 1024;

/// `secure_entry` kinds.
pub const KIND_MP_CREATE: u8 = 0;
pub const KIND_MP_ENTRY: u8 = 1;
pub const KIND_MP_CHANGE: u8 = 2;
pub const KIND_RK_ENTRY: u8 = 3;

/// Callback results: the user submitted / acknowledged (0), anything else
/// is a cancel.
pub const SUBMITTED: i32 = 0;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ov0Callbacks {
    pub ctx: *mut c_void,
    /// Crossing (b). `kind` as above; the first secret into `a`, the second
    /// (an MP change's new MP) into `b`; lengths through `a_len`/`b_len`.
    pub secure_entry: extern "C" fn(ctx: *mut c_void, kind: u8, timeout_ms: u64, a: *mut u8, a_len: *mut usize, b: *mut u8, b_len: *mut usize, cap: usize) -> i32,
    /// Crossing (c): the 24 words and the sheet's three lines; 0 only when
    /// the user confirmed they saved it.
    pub recovery_sheet: extern "C" fn(ctx: *mut c_void, words: *const u8, words_len: usize, checkpoint: *const c_char, recovery: *const c_char, reason: *const c_char, timeout_ms: u64) -> i32,
    pub presence: extern "C" fn(ctx: *mut c_void, reason: *const c_char) -> bool,
    pub capture_suppressed: extern "C" fn(ctx: *mut c_void, surface: *const c_char) -> bool,
    pub event: extern "C" fn(ctx: *mut c_void, json: *const u8, len: usize),
}

/// The table, shared by the four services. Swift owns `ctx` and makes
/// every callback safe to call from any thread.
#[derive(Clone, Copy)]
pub struct Shared(Ov0Callbacks);

// SAFETY: the callback contract (phase-f2b-ffi.md) requires a thread-safe
// `ctx`; the table itself is plain function pointers.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

fn c(s: &str) -> CString {
    CString::new(s.replace('\0', " ")).expect("no NUL left")
}

impl Shared {
    pub fn new(cb: Ov0Callbacks) -> Shared {
        Shared(cb)
    }

    pub fn deps(self) -> Deps {
        let s = Arc::new(self);
        Deps { panel: s.clone(), la: s.clone(), capture: s.clone(), events: s }
    }
}

impl PanelRunner for Shared {
    fn run(&self, req: PanelRequest, timeout: Duration) -> PanelOutcome {
        let kind = match req {
            PanelRequest::MpCreate => KIND_MP_CREATE,
            PanelRequest::MpEntry => KIND_MP_ENTRY,
            PanelRequest::MpChange => KIND_MP_CHANGE,
            PanelRequest::RkEntry => KIND_RK_ENTRY,
        };
        let mut a = Zeroizing::new(vec![0u8; ENTRY_CAP]);
        let mut b = Zeroizing::new(vec![0u8; ENTRY_CAP]);
        let (mut a_len, mut b_len) = (0usize, 0usize);
        let rc = (self.0.secure_entry)(self.0.ctx, kind, timeout.as_millis() as u64, a.as_mut_ptr(), &mut a_len, b.as_mut_ptr(), &mut b_len, ENTRY_CAP);
        if rc != SUBMITTED || a_len > ENTRY_CAP || b_len > ENTRY_CAP {
            return PanelOutcome::Cancelled;
        }
        a.truncate(a_len);
        b.truncate(b_len);
        match req {
            PanelRequest::MpChange => PanelOutcome::SubmittedChange(a, b),
            _ => PanelOutcome::Submitted(a),
        }
    }

    fn show_recovery_key(&self, sheet: &RecoverySheet, timeout: Duration) -> PanelOutcome {
        let (cp, rec, why) = (c(&sheet.checkpoint), c(&sheet.recovery), c(sheet.reason.line()));
        let rc = (self.0.recovery_sheet)(self.0.ctx, sheet.words.as_ptr(), sheet.words.len(), cp.as_ptr(), rec.as_ptr(), why.as_ptr(), timeout.as_millis() as u64);
        if rc == SUBMITTED { PanelOutcome::Acknowledged } else { PanelOutcome::Cancelled }
    }
}

impl PresenceChecker for Shared {
    fn check(&self, reason: &str) -> bool {
        (self.0.presence)(self.0.ctx, c(reason).as_ptr())
    }
}

impl CaptureChecker for Shared {
    fn suppressed(&self, surface: &str) -> bool {
        (self.0.capture_suppressed)(self.0.ctx, c(surface).as_ptr())
    }
}

impl EventSink for Shared {
    fn emit(&self, event: Value) {
        let bytes = serde_json::to_vec(&event).unwrap_or_default();
        (self.0.event)(self.0.ctx, bytes.as_ptr(), bytes.len());
    }
}
