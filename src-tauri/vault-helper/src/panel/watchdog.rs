//! Modal-session watchdog: the only way to dismiss a visible panel from
//! outside it (lock preemption, §13.3 panel timeout, debug autoshow).
//!
//! The panel runs inside `runModalForWindow`, entered from a block on the
//! main dispatch queue. The main queue is serial, so any dismissal
//! dispatched to it would wait behind the very block it is meant to end —
//! the panel would stay on screen after the op had already reported it
//! gone. Instead, a CFRunLoopTimer in the common modes (which include the
//! modal-panel mode) polls the job's abort flag every 50 ms from inside the
//! modal session and ends it with `abortModal`, the AppKit call meant for
//! stopping a modal loop from a timer.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(debug_assertions)]
use std::time::{Duration, Instant};

use core_foundation::base::TCFType;
use core_foundation::date::CFAbsoluteTimeGetCurrent;
use core_foundation::runloop::{
    kCFRunLoopCommonModes, CFRunLoop, CFRunLoopTimer, CFRunLoopTimerContext,
    CFRunLoopTimerInvalidate, CFRunLoopTimerRef,
};

use super::appkit;

const TICK_SECS: f64 = 0.05;

/// Debug autoshow: dismiss after this long, sampling the UI-02 probe first.
#[cfg(debug_assertions)]
const AUTOSHOW_AFTER: Duration = Duration::from_millis(400);

struct Ctx {
    abort: Arc<AtomicBool>,
    fired: AtomicBool,
    #[cfg(debug_assertions)]
    autoshow_since: Option<Instant>,
}

/// Live for the duration of one modal session; dropping it invalidates the
/// timer before the context it points at is freed.
pub(super) struct Watchdog {
    timer: CFRunLoopTimer,
    _ctx: Box<Ctx>,
}

impl Watchdog {
    /// Install on the main run loop. Main-thread only.
    pub(super) fn start(abort: Arc<AtomicBool>) -> Watchdog {
        let ctx = Box::new(Ctx {
            abort,
            fired: AtomicBool::new(false),
            #[cfg(debug_assertions)]
            autoshow_since: (std::env::var("OV0_VAULT_PANEL_SCRIPT").as_deref() == Ok("autoshow"))
                .then(Instant::now),
        });
        let mut context = CFRunLoopTimerContext {
            version: 0,
            info: &*ctx as *const Ctx as *mut c_void,
            retain: None,
            release: None,
            copyDescription: None,
        };
        // SAFETY: CFAbsoluteTimeGetCurrent is a pure clock read.
        let first = unsafe { CFAbsoluteTimeGetCurrent() } + TICK_SECS;
        let timer = CFRunLoopTimer::new(first, TICK_SECS, 0, 0, tick, &mut context);
        // SAFETY: kCFRunLoopCommonModes is an immutable framework constant.
        CFRunLoop::get_main().add_timer(&timer, unsafe { kCFRunLoopCommonModes });
        Watchdog { timer, _ctx: ctx }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        // SAFETY: valid timer owned by self; after this no callout can run.
        unsafe { CFRunLoopTimerInvalidate(self.timer.as_concrete_TypeRef()) };
    }
}

extern "C" fn tick(_timer: CFRunLoopTimerRef, info: *mut c_void) {
    // SAFETY: `info` is the Box<Ctx> held by the live Watchdog; the timer
    // is invalidated in Drop before the box is freed.
    let ctx = unsafe { &*(info as *const Ctx) };
    #[cfg(debug_assertions)]
    if let Some(since) = ctx.autoshow_since {
        if since.elapsed() >= AUTOSHOW_AFTER && !ctx.abort.load(Ordering::SeqCst) {
            appkit::probe_current();
            ctx.abort.store(true, Ordering::SeqCst);
        }
    }
    if ctx.abort.load(Ordering::SeqCst) && !ctx.fired.swap(true, Ordering::SeqCst) {
        appkit::abort_current();
    }
}
