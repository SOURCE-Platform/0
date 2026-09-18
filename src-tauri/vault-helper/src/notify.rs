//! System lock triggers (§1.6 auto-lock list): system sleep, screen lock,
//! and session switch. Observers live on a dedicated thread with an
//! NSRunLoop (Cocoa delivers workspace notifications to the registering
//! thread's run loop); each notification flips an atomic flag, nothing
//! more — LA panels, crypto, and zeroization never run inside a
//! notification callback. The executor tick drains the flags and performs
//! the actual lock (§13.3 zeroize path).

use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2_app_kit::{
    NSWorkspace, NSWorkspaceSessionDidResignActiveNotification,
    NSWorkspaceWillSleepNotification,
};
use objc2_foundation::{NSDistributedNotificationCenter, NSNotification, NSRunLoop, NSString};

use crate::vault::LockReason;

/// Pending system lock triggers, shared between the notify thread and
/// the executor tick.
pub struct LockTriggers {
    sleep: AtomicBool,
    screen_lock: AtomicBool,
}

impl LockTriggers {
    pub fn new() -> Arc<LockTriggers> {
        Arc::new(LockTriggers {
            sleep: AtomicBool::new(false),
            screen_lock: AtomicBool::new(false),
        })
    }

    /// Drain pending triggers into at most one lock reason. Sleep wins
    /// over screen lock when both fired (sleep subsumes it); the reason
    /// string only feeds the `locked` event's `reason` field (§1.5).
    pub fn take(&self) -> Option<LockReason> {
        if self.sleep.swap(false, Ordering::SeqCst) {
            let _ = self.screen_lock.swap(false, Ordering::SeqCst);
            return Some(LockReason::Sleep);
        }
        if self.screen_lock.swap(false, Ordering::SeqCst) {
            return Some(LockReason::ScreenLock);
        }
        None
    }

    /// Test/gate hook: raise a trigger without going through Cocoa.
    pub fn raise_sleep(&self) {
        self.sleep.store(true, Ordering::SeqCst);
    }

    pub fn raise_screen_lock(&self) {
        self.screen_lock.store(true, Ordering::SeqCst);
    }
}

/// Distributed-notification name for the loginwindow screen lock. This
/// is the documented mechanism for "screen lock" on macOS (no public
/// constant exists).
const SCREEN_IS_LOCKED: &str = "com.apple.screenIsLocked";

/// Register the three observers and run this thread's run loop forever.
/// Called once at helper startup; observers live for the process.
pub fn install(triggers: Arc<LockTriggers>) {
    std::thread::spawn(move || run_observer_loop(triggers));
}

fn run_observer_loop(triggers: Arc<LockTriggers>) {
    // SAFETY: all Cocoa statics/methods used per their main-thread-agnostic
    // contracts (NSWorkspace centers and distributed notifications are
    // documented thread-safe); the run loop keeps this thread alive to
    // receive deliveries. Extern statics are immutable process constants.
    unsafe {
        let workspace = NSWorkspace::sharedWorkspace();
        let center = workspace.notificationCenter();

        let sleep_block = flag_block(&triggers, Trigger::Sleep);
        let resign_block = flag_block(&triggers, Trigger::ScreenLock);
        // Observer tokens must stay retained for observation to continue;
        // this function never returns, so the bindings live forever.
        let _sleep_observer: Retained<NSObject> = center
            .addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceWillSleepNotification),
                None,
                None,
                &sleep_block,
            );
        let _resign_observer: Retained<NSObject> = center
            .addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceSessionDidResignActiveNotification),
                None,
                None,
                &resign_block,
            );

        let dist = NSDistributedNotificationCenter::defaultCenter();
        let locked_block = flag_block(&triggers, Trigger::ScreenLock);
        let locked_name = NSString::from_str(SCREEN_IS_LOCKED);
        let _locked_observer: Retained<NSObject> = dist
            .addObserverForName_object_queue_usingBlock(
                Some(&locked_name),
                None,
                None,
                &locked_block,
            );

        NSRunLoop::currentRunLoop().run();
    }
}

enum Trigger {
    Sleep,
    ScreenLock,
}

fn flag_block(
    triggers: &Arc<LockTriggers>,
    kind: Trigger,
) -> RcBlock<dyn Fn(NonNull<NSNotification>)> {
    let triggers = Arc::clone(triggers);
    RcBlock::new(move |_note: NonNull<NSNotification>| match kind {
        Trigger::Sleep => triggers.raise_sleep(),
        Trigger::ScreenLock => triggers.raise_screen_lock(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_prefers_sleep_and_clears_both() {
        let triggers = LockTriggers::new();
        assert_eq!(triggers.take(), None);
        triggers.raise_screen_lock();
        assert_eq!(triggers.take(), Some(LockReason::ScreenLock));
        assert_eq!(triggers.take(), None);

        triggers.raise_screen_lock();
        triggers.raise_sleep();
        assert_eq!(triggers.take(), Some(LockReason::Sleep));
        assert_eq!(triggers.take(), None, "both flags drained by sleep");
    }
}
