//! Bank login happens outside SOURCE, so suppress keyboard capture globally
//! until the authorization/import command finishes, not just in our own window.
use std::sync::atomic::{AtomicUsize, Ordering};

static BANK_AUTH_OPEN: AtomicUsize = AtomicUsize::new(0);

pub fn bank_authorization_active() -> bool {
    BANK_AUTH_OPEN.load(Ordering::SeqCst) > 0
}

pub struct BankAuthorizationGuard;

impl BankAuthorizationGuard {
    pub fn new() -> Self {
        BANK_AUTH_OPEN.fetch_add(1, Ordering::SeqCst);
        super::capture_exclusions::sensitive_surface_shown();
        Self
    }
}

impl Drop for BankAuthorizationGuard {
    fn drop(&mut self) {
        BANK_AUTH_OPEN
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                Some(n.saturating_sub(1))
            })
            .ok();
        super::capture_exclusions::sensitive_surface_hidden();
    }
}
