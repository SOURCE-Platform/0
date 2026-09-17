//! Central registry of SOURCE surfaces that must never be recorded, OCR'd,
//! indexed, or keylogged.
//!
//! The credential vault and import windows (see
//! `docs/security/credential-vault-security-architecture.md` §15.5) are the
//! intended consumers. Nothing registers a surface yet, so current capture
//! behavior is unchanged; the mechanism lands first so the vault UI can rely
//! on it from its first commit.
//!
//! Design rule: when detection is ambiguous, fail toward not recording.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

/// Exact window titles of SOURCE-owned windows whose contents are sensitive.
fn excluded_titles() -> &'static Mutex<HashSet<String>> {
    static TITLES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    TITLES.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Number of registered-sensitive surfaces currently visible. A counter (not
/// a bool) so overlapping surfaces can't cancel each other's protection.
static SENSITIVE_SURFACES_OPEN: AtomicUsize = AtomicUsize::new(0);

/// Register a SOURCE window title whose contents must never be captured.
pub fn register_excluded_window_title(title: &str) {
    if let Ok(mut titles) = excluded_titles().lock() {
        titles.insert(title.to_string());
    }
}

pub fn unregister_excluded_window_title(title: &str) {
    if let Ok(mut titles) = excluded_titles().lock() {
        titles.remove(title);
    }
}

pub fn is_excluded_window_title(title: &str) -> bool {
    excluded_titles()
        .lock()
        .map(|titles| titles.contains(title))
        .unwrap_or(true) // lock poisoned: fail closed
}

/// Called when a sensitive surface (e.g. a vault window) becomes visible.
pub fn sensitive_surface_shown() {
    SENSITIVE_SURFACES_OPEN.fetch_add(1, Ordering::SeqCst);
}

/// Called when a sensitive surface is hidden or closed. Saturates at zero so
/// a stray "hidden" can never underflow the counter.
pub fn sensitive_surface_hidden() {
    SENSITIVE_SURFACES_OPEN
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
            Some(n.saturating_sub(1))
        })
        .ok();
}

pub fn sensitive_surface_visible() -> bool {
    SENSITIVE_SURFACES_OPEN.load(Ordering::SeqCst) > 0
}

/// Whether screen/OCR frame capture must pause right now.
///
/// The current macOS capture backend records the whole display
/// (`CGDisplay::image`), so per-window exclusion is approximated by dropping
/// frames while any sensitive surface is visible. Fail closed on purpose.
pub fn screen_capture_suppressed() -> bool {
    sensitive_surface_visible()
}

/// Single choke-point decision for keystroke recording.
///
/// Suppress when any signal says the input context is sensitive:
/// - the focused UI element was classified as a password/secure field,
/// - macOS reports Secure Event Input is active (browsers enable it for
///   password fields; event taps are also OS-suppressed then, but other
///   capture paths and future listeners must gate explicitly),
/// - SOURCE's own sensitive surface is frontmost (typing into the vault).
pub fn should_record_keystroke(
    field_marked_sensitive: bool,
    secure_event_input_active: bool,
    own_sensitive_surface_frontmost: bool,
) -> bool {
    !(field_marked_sensitive
        || secure_event_input_active
        || own_sensitive_surface_frontmost)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The visibility counter is process-global, so these checks live in one
    /// test to avoid interleaving under parallel test execution.
    #[test]
    fn surface_counter_tracks_open_and_close() {
        let before = SENSITIVE_SURFACES_OPEN.load(Ordering::SeqCst);
        sensitive_surface_shown();
        sensitive_surface_shown();
        assert_eq!(SENSITIVE_SURFACES_OPEN.load(Ordering::SeqCst), before + 2);
        assert!(sensitive_surface_visible());
        assert!(screen_capture_suppressed());
        sensitive_surface_hidden();
        sensitive_surface_hidden();
        assert_eq!(SENSITIVE_SURFACES_OPEN.load(Ordering::SeqCst), before);
        // Extra hidden calls must not underflow below the pre-test value.
        sensitive_surface_hidden();
        assert!(SENSITIVE_SURFACES_OPEN.load(Ordering::SeqCst) <= before);
    }

    #[test]
    fn title_registry_registers_and_unregisters() {
        let title = "capture-exclusions-test-window";
        assert!(!is_excluded_window_title(title));
        register_excluded_window_title(title);
        assert!(is_excluded_window_title(title));
        unregister_excluded_window_title(title);
        assert!(!is_excluded_window_title(title));
    }

    #[test]
    fn keystroke_recorded_only_when_nothing_is_sensitive() {
        assert!(should_record_keystroke(false, false, false));
    }

    #[test]
    fn keystroke_suppressed_for_sensitive_field() {
        assert!(!should_record_keystroke(true, false, false));
    }

    #[test]
    fn keystroke_suppressed_while_secure_event_input_active() {
        assert!(!should_record_keystroke(false, true, false));
    }

    #[test]
    fn keystroke_suppressed_when_own_sensitive_surface_frontmost() {
        assert!(!should_record_keystroke(false, false, true));
    }

    #[test]
    fn keystroke_suppressed_when_any_signal_is_set() {
        for sensitive in [true, false] {
            for secure in [true, false] {
                for own_frontmost in [true, false] {
                    let expected = !(sensitive || secure || own_frontmost);
                    assert_eq!(
                        should_record_keystroke(sensitive, secure, own_frontmost),
                        expected
                    );
                }
            }
        }
    }
}
