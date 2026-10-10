//! The Swift callback table (spec v0.5 §22.2): SOURCE Vault's secure entry,
//! Recovery Key sheet, presence check, capture check and event sink, as
//! the engine's `Deps` services. Secrets cross only here and only as the
//! catalogue allows: MP / RK words **in** (written by Swift into
//! engine-owned zeroizing buffers, crossing b) and RK words **out** to the
//! sheet (crossing c).

use std::ffi::{c_char, c_void, CString};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use vault_engine::vault::{
    CaptureChecker, Deps, EventSink, PanelOutcome, PanelRequest, PanelRunner, PresenceChecker, RecoverySheet,
};
use zeroize::Zeroizing;

/// Longest secret a secure-entry field may hand over (an MP or 24 words).
pub const ENTRY_CAP: usize = 4096;
/// The Mac panel's rule for a new master password (`panel/form.rs`): at
/// least this many characters of valid UTF-8 (review SEC-I2).
pub const MIN_MP_CHARS: usize = 8;

/// `secure_entry` kinds.
pub const KIND_MP_CREATE: u8 = 0;
pub const KIND_MP_ENTRY: u8 = 1;
pub const KIND_MP_CHANGE: u8 = 2;
pub const KIND_RK_ENTRY: u8 = 3;
/// The master password for adopting a key change (§2.7, F.2d).
pub const KIND_MP_ADOPT: u8 = 4;

/// Callback results: the user submitted / acknowledged (0), anything else
/// is a cancel.
pub const SUBMITTED: i32 = 0;

type EntryFn = extern "C" fn(ctx: *mut c_void, kind: u8, timeout_ms: u64, a: *mut u8, a_len: *mut usize, b: *mut u8, b_len: *mut usize, cap: usize) -> i32;
type SheetFn = extern "C" fn(ctx: *mut c_void, words: *const u8, words_len: usize, checkpoint: *const c_char, recovery: *const c_char, reason: *const c_char, timeout_ms: u64) -> i32;
type AskFn = extern "C" fn(ctx: *mut c_void, text: *const c_char) -> bool;
type EventFn = extern "C" fn(ctx: *mut c_void, json: *const u8, len: usize);

/// The table Swift passes. Every member must be non-null; a table with a
/// missing one is refused at `ov0_engine_open` (review VER-O6).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ov0Callbacks {
    pub ctx: *mut c_void,
    /// Crossing (b). `kind` as above; the first secret into `a`, the second
    /// (an MP change's new MP) into `b`; lengths through `a_len`/`b_len`.
    pub secure_entry: Option<EntryFn>,
    /// Crossing (c): the 24 words and the sheet's three lines; 0 only when
    /// the user confirmed they saved it.
    pub recovery_sheet: Option<SheetFn>,
    pub presence: Option<AskFn>,
    pub capture_suppressed: Option<AskFn>,
    pub event: Option<EventFn>,
}

/// The validated table.
#[derive(Clone, Copy)]
struct Table {
    ctx: *mut c_void,
    secure_entry: EntryFn,
    recovery_sheet: SheetFn,
    presence: AskFn,
    capture_suppressed: AskFn,
    event: EventFn,
}

impl Table {
    fn from(cb: &Ov0Callbacks) -> Option<Table> {
        Some(Table {
            ctx: cb.ctx,
            secure_entry: cb.secure_entry?,
            recovery_sheet: cb.recovery_sheet?,
            presence: cb.presence?,
            capture_suppressed: cb.capture_suppressed?,
            event: cb.event?,
        })
    }
}

/// The table, shared by the four services, plus the events waiting to be
/// delivered. Events are queued and handed to Swift only by `flush`, which
/// the entry points call with no engine lock held — so a Swift event
/// handler can never deadlock a lock (review SEC-I4).
pub struct Shared {
    cb: Table,
    queue: Mutex<Vec<Value>>,
    /// Held across take-and-deliver, so events reach Swift in the order
    /// they happened even when two threads flush (review VER-O16). Never
    /// an engine lock.
    delivery: Mutex<()>,
}

// SAFETY: the callback contract (phase-f2b-ffi.md §2) requires a
// thread-safe `ctx`; the table itself is plain function pointers.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

fn c(s: &str) -> CString {
    CString::new(s.replace('\0', " ")).expect("no NUL left")
}

/// A new master password the Mac panel would also accept.
fn acceptable_new_mp(mp: &[u8]) -> bool {
    std::str::from_utf8(mp).is_ok_and(|s| s.chars().count() >= MIN_MP_CHARS)
}

impl Shared {
    /// `None` when any callback is missing.
    pub fn new(cb: &Ov0Callbacks) -> Option<Arc<Shared>> {
        Some(Arc::new(Shared { cb: Table::from(cb)?, queue: Mutex::new(Vec::new()), delivery: Mutex::new(()) }))
    }

    pub fn deps(self: &Arc<Self>) -> Deps {
        Deps { panel: self.clone(), la: self.clone(), capture: self.clone(), events: self.clone() }
    }

    /// Deliver the queued events, in order. Call with no engine lock held.
    pub fn flush(&self) {
        let _order = self.delivery.lock().unwrap_or_else(|p| p.into_inner());
        let events = std::mem::take(&mut *self.queue.lock().unwrap_or_else(|p| p.into_inner()));
        for event in events {
            let bytes = serde_json::to_vec(&event).unwrap_or_default();
            (self.cb.event)(self.cb.ctx, bytes.as_ptr(), bytes.len());
        }
    }
}

impl PanelRunner for Shared {
    fn run(&self, req: PanelRequest, timeout: Duration) -> PanelOutcome {
        let kind = match req {
            // The phone never authorizes an enrollment (F2-D4 is reserved).
            PanelRequest::EnrollConfirm => return PanelOutcome::Cancelled,
            PanelRequest::MpCreate => KIND_MP_CREATE,
            PanelRequest::MpEntry => KIND_MP_ENTRY,
            PanelRequest::MpChange => KIND_MP_CHANGE,
            PanelRequest::RkEntry => KIND_RK_ENTRY,
            PanelRequest::MpAdopt => KIND_MP_ADOPT,
        };
        let mut a = Zeroizing::new(vec![0u8; ENTRY_CAP]);
        let mut b = Zeroizing::new(vec![0u8; ENTRY_CAP]);
        let (mut a_len, mut b_len) = (0usize, 0usize);
        let rc = (self.cb.secure_entry)(self.cb.ctx, kind, timeout.as_millis() as u64, a.as_mut_ptr(), &mut a_len, b.as_mut_ptr(), &mut b_len, ENTRY_CAP);
        if rc != SUBMITTED || a_len > ENTRY_CAP || b_len > ENTRY_CAP {
            return PanelOutcome::Cancelled;
        }
        a.truncate(a_len);
        b.truncate(b_len);
        // The engine never stores a new MP the Mac panel would refuse.
        match req {
            PanelRequest::MpChange if acceptable_new_mp(&b) => PanelOutcome::SubmittedChange(a, b),
            PanelRequest::MpCreate if acceptable_new_mp(&a) => PanelOutcome::Submitted(a),
            PanelRequest::MpEntry | PanelRequest::MpAdopt | PanelRequest::RkEntry => PanelOutcome::Submitted(a),
            _ => PanelOutcome::Cancelled,
        }
    }

    fn show_recovery_key(&self, sheet: &RecoverySheet, timeout: Duration) -> PanelOutcome {
        let (cp, rec, why) = (c(&sheet.checkpoint), c(&sheet.recovery), c(sheet.reason.line()));
        let rc = (self.cb.recovery_sheet)(self.cb.ctx, sheet.words.as_ptr(), sheet.words.len(), cp.as_ptr(), rec.as_ptr(), why.as_ptr(), timeout.as_millis() as u64);
        if rc == SUBMITTED { PanelOutcome::Acknowledged } else { PanelOutcome::Cancelled }
    }
}

impl PresenceChecker for Shared {
    fn check(&self, reason: &str) -> bool {
        (self.cb.presence)(self.cb.ctx, c(reason).as_ptr())
    }
}

impl CaptureChecker for Shared {
    fn suppressed(&self, surface: &str) -> bool {
        (self.cb.capture_suppressed)(self.cb.ctx, c(surface).as_ptr())
    }
}

impl EventSink for Shared {
    fn emit(&self, event: Value) {
        self.queue.lock().unwrap_or_else(|p| p.into_inner()).push(event);
    }
}
