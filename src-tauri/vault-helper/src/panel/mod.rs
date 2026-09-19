//! Helper-owned secure panel runner (§1.7). Two execution modes:
//!
//! - **Real**: the request is marshaled to the AppKit main queue via
//!   `dispatch_async_f`; `panel::appkit::present` builds and runs the
//!   modal NSPanel there. The caller (an ops-executor thread) blocks on a
//!   channel, bounded by the §13.3 120 s timeout, polling the server's
//!   cancel flag so `lock` preempts a hanging panel.
//! - **Scripted** (debug builds only): `OV0_VAULT_PANEL_SCRIPT` answers
//!   without UI so the gate/CI can drive flows headlessly. Release builds
//!   compile the env handling out entirely.
//!
//! MP/RK secrets cross back in zeroizing buffers only (§2.11); the
//! executor never logs them (§15).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::vault::{PanelOutcome, PanelRequest, PanelRunner};

pub mod appkit;
mod form;
pub mod rk_sheet;
mod watchdog;

// libdispatch main-queue trampoline plumbing. `dispatch_get_main_queue()`
// is a header-inline over the `_dispatch_main_q` global, so we link the
// global directly; `dispatch_async_f` is a real libSystem export.
extern "C" {
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        queue: *mut std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: extern "C" fn(*mut std::ffi::c_void),
    );
}

fn main_queue() -> *mut std::ffi::c_void {
    // SAFETY: process-global queue object, present before main() runs.
    unsafe { &_dispatch_main_q as *const std::ffi::c_void as *mut std::ffi::c_void }
}

struct PanelJob {
    req: PanelRequest,
    reply: Sender<PanelOutcome>,
    /// Per-job dismissal flag, polled by the modal-session watchdog.
    abort: Arc<AtomicBool>,
}

/// How long a preempted run waits for the panel to actually leave the
/// screen before reporting Cancelled. The executor emits
/// `secure_panel_visible:false` on return, and the main app drops capture
/// suppression on that event — it must not precede the real dismissal.
const DISMISS_WAIT: Duration = Duration::from_secs(2);

/// Runs on the AppKit main queue. Presents the panel and reports the
/// outcome back to the waiting executor thread.
extern "C" fn present_trampoline(ctx: *mut std::ffi::c_void) {
    // SAFETY: `ctx` is a Box<PanelJob> produced exactly once by
    // HelperPanel::run; reconstructing it here returns ownership to us.
    let job = unsafe { Box::from_raw(ctx.cast::<PanelJob>()) };
    let outcome = appkit::present(&job.req, job.abort.clone());
    let _ = job.reply.send(outcome);
}

struct SheetJob {
    sheet: crate::vault::RecoverySheet,
    reply: Sender<bool>,
    abort: Arc<AtomicBool>,
}

/// Main queue: present the Recovery Key window, report acknowledgement.
extern "C" fn sheet_trampoline(ctx: *mut std::ffi::c_void) {
    // SAFETY: `ctx` is a Box<SheetJob> produced exactly once by
    // HelperPanel::show_recovery_key.
    let job = unsafe { Box::from_raw(ctx.cast::<SheetJob>()) };
    let ok = rk_sheet::present(&job.sheet, job.abort.clone());
    let _ = job.reply.send(ok);
    // job (and its Zeroizing words) drops here, on the main thread.
}

/// Wait for a main-queue job, aborting it on lock/timeout and waiting for
/// the real dismissal before reporting (see DISMISS_WAIT).
fn wait_job<T>(rx: &std::sync::mpsc::Receiver<T>, abort: &AtomicBool, cancel: &AtomicBool, timeout: Duration) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(v) => return Some(v),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return None,
        }
        if cancel.load(Ordering::SeqCst) || Instant::now() >= deadline {
            abort.store(true, Ordering::SeqCst);
            let _ = rx.recv_timeout(DISMISS_WAIT);
            return None;
        }
    }
}

fn dispatch_main(job: extern "C" fn(*mut std::ffi::c_void), ctx: *mut std::ffi::c_void) {
    // SAFETY: main_queue() is the process-wide main queue; `ctx`
    // ownership transfers to the trampoline (Box::from_raw there).
    unsafe { dispatch_async_f(main_queue(), ctx, job) };
}

/// Production panel runner. `cancel` is the server's op-cancel flag (set
/// by an explicit `lock` while a panel is up).
pub struct HelperPanel {
    pub cancel: Arc<AtomicBool>,
}

impl PanelRunner for HelperPanel {
    fn run(&self, req: PanelRequest, timeout: Duration) -> PanelOutcome {
        #[cfg(debug_assertions)]
        if let Some(outcome) = scripted(&req) {
            return outcome;
        }
        let (tx, rx) = channel();
        let abort = Arc::new(AtomicBool::new(false));
        let job = Box::into_raw(Box::new(PanelJob { req, reply: tx, abort: abort.clone() }));
        dispatch_main(present_trampoline, job.cast());
        wait_job(&rx, &abort, &self.cancel, timeout).unwrap_or(PanelOutcome::Cancelled)
    }

    fn show_recovery_key(&self, sheet: &crate::vault::RecoverySheet, timeout: Duration) -> PanelOutcome {
        #[cfg(debug_assertions)]
        if let Some(outcome) = scripted_sheet(sheet) {
            return outcome;
        }
        let (tx, rx) = channel();
        let abort = Arc::new(AtomicBool::new(false));
        let job = SheetJob {
            sheet: crate::vault::RecoverySheet { words: sheet.words.clone(), checkpoint: sheet.checkpoint.clone() },
            reply: tx,
            abort: abort.clone(),
        };
        dispatch_main(sheet_trampoline, Box::into_raw(Box::new(job)).cast());
        match wait_job(&rx, &abort, &self.cancel, timeout) {
            Some(true) => PanelOutcome::Acknowledged,
            _ => PanelOutcome::Cancelled,
        }
    }
}

/// Debug-only: the last Recovery Key a scripted run "showed", so a
/// scripted RK entry can type it back (gate E2E) without the words ever
/// leaving the helper process or appearing in any output.
#[cfg(debug_assertions)]
static LAST_SHOWN_RK: std::sync::Mutex<Option<zeroize::Zeroizing<String>>> = std::sync::Mutex::new(None);

#[cfg(debug_assertions)]
fn scripted_sheet(sheet: &crate::vault::RecoverySheet) -> Option<PanelOutcome> {
    // OV0_VAULT_SHEET_SCRIPT=autoshow-print: show the REAL window (gate
    // UI-04 evidence) even when MP panels are scripted.
    if std::env::var("OV0_VAULT_SHEET_SCRIPT").is_ok_and(|v| v.starts_with("autoshow")) {
        return None;
    }
    let script = std::env::var("OV0_VAULT_PANEL_SCRIPT").ok()?;
    if script.starts_with("autoshow") {
        return None;
    }
    if script == "cancel" {
        return Some(PanelOutcome::Cancelled);
    }
    *LAST_SHOWN_RK.lock().unwrap_or_else(|e| e.into_inner()) = Some(sheet.words.clone());
    Some(PanelOutcome::Acknowledged)
}

/// Debug-only scripted outcomes. `OV0_VAULT_PANEL_SCRIPT`:
/// - `submit:<mp>` or `submit:<mp>,<new>` — immediate submission; a
///   Recovery Key window is acknowledged, and RK entry types back the
///   last Recovery Key shown
/// - `cancel` — immediate cancellation
/// - `autoshow` — present the REAL panel and auto-dismiss it after
///   ~400 ms (gate evidence for UI-01/UI-02 on a live window)
#[cfg(debug_assertions)]
fn scripted(req: &PanelRequest) -> Option<PanelOutcome> {
    let script = std::env::var("OV0_VAULT_PANEL_SCRIPT").ok()?;
    if script.starts_with("autoshow") {
        return None; // fall through to the real panel; appkit applies the timer
    }
    if script == "cancel" {
        return Some(PanelOutcome::Cancelled);
    }
    let body = script.strip_prefix("submit:")?;
    let secret = |s: &str| crate::crypto::secret::SecretVec::new(s.as_bytes().to_vec());
    let mut parts = body.splitn(2, ',');
    let first = parts.next().unwrap_or("");
    let second = parts.next().unwrap_or("");
    match req {
        PanelRequest::MpCreate | PanelRequest::MpEntry => {
            Some(PanelOutcome::Submitted(secret(first)))
        }
        PanelRequest::MpChange => Some(PanelOutcome::SubmittedChange(secret(first), secret(second))),
        PanelRequest::RkEntry => {
            let shown = LAST_SHOWN_RK.lock().unwrap_or_else(|e| e.into_inner()).clone();
            Some(match shown {
                Some(words) => PanelOutcome::Submitted(crate::crypto::secret::SecretVec::new(words.as_bytes().to_vec())),
                None => PanelOutcome::Cancelled,
            })
        }
    }
}
