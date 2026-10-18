//! Why a lock happened, and the lock request flag (§13.3; final review
//! SPEC-I10).

/// A lock is waiting for the core (§13.3: an explicit lock is never held
/// up by a long op). Set by the lock path *before* it takes the mutex; a
/// long op holding the mutex — the Dashlane import — checks it between rows
/// and stops early (final review SPEC-I10). `VaultCore::lock` clears it.
pub static LOCK_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Ask for the lock ahead of taking the core mutex.
pub fn request_lock() {
    LOCK_REQUESTED.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockReason {
    Explicit,
    Timeout,
    Sleep,
    ScreenLock,
    PanelTimeout,
    Fatal,
}

impl LockReason {
    pub fn as_str(self) -> &'static str {
        match self {
            LockReason::Explicit => "explicit",
            LockReason::Timeout => "timeout",
            LockReason::Sleep => "sleep",
            LockReason::ScreenLock => "screen_lock",
            LockReason::PanelTimeout => "panel_timeout",
            LockReason::Fatal => "fatal",
        }
    }
}
