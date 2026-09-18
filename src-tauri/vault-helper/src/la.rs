//! LocalAuthentication presence checks (spec §6.4): exactly one
//! `LAPolicy.deviceOwnerAuthentication` call per gated op. Apple's own
//! behavior gives the intended UX on both Mac classes (Touch ID sheet
//! with password fallback, or the native login-password sheet on
//! clamshell) — `...WithBiometrics` is never used for vault ops because
//! it fails instead of offering the password path (§6.4 step 3).
//!
//! Fail-closed: any evaluation error, denial, or cancellation maps to
//! `false` (the op layer answers PRESENCE_DENIED).

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LAContext, LAPolicy};

use crate::vault::PresenceChecker;

/// Outer bound on one presence evaluation. LA sheets have no documented
/// timeout; a wedged sheet must not wedge the ops executor, so a stuck
/// evaluation fails closed after 90 s (under the §13.3 120 s op abort).
const LA_TIMEOUT: Duration = Duration::from_secs(90);

pub struct LaPresence;

impl PresenceChecker for LaPresence {
    fn check(&self, reason: &str) -> bool {
        // Debug-only stub for gate/CI runs where no interactive user
        // exists. Compiled out of release builds entirely.
        #[cfg(debug_assertions)]
        if let Ok(stub) = std::env::var("OV0_VAULT_LA_STUB") {
            return stub == "allow";
        }
        evaluate(reason)
    }
}

fn evaluate(reason: &str) -> bool {
    let context = unsafe { LAContext::new() };
    // canEvaluatePolicy distinguishes "policy unusable" up front; treat
    // any error as denial (fail closed).
    if unsafe { context.canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthentication) }.is_err() {
        return false;
    }
    let (tx, rx) = mpsc::channel();
    let reply = RcBlock::new(move |success: Bool, _error: *mut NSError| {
        let _ = tx.send(success.as_bool());
    });
    let reason = NSString::from_str(reason);
    // SAFETY: `context` is a live LAContext; `reply` is a valid block
    // whose captures (the sender) outlive the call because we wait below.
    unsafe {
        context.evaluatePolicy_localizedReason_reply(
            LAPolicy::DeviceOwnerAuthentication,
            &reason,
            &reply,
        );
    }
    let result = rx.recv_timeout(LA_TIMEOUT).unwrap_or(false);
    // SAFETY: balances the evaluation; safe on a completed context.
    unsafe { context.invalidate() };
    result
}
