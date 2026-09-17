//! Capture-exclusion controls for SOURCE-owned sensitive surfaces.
//!
//! These commands are the generic mechanism the credential vault and import
//! windows will use to remove themselves from screen recording, OCR,
//! indexing, and keyboard capture. They exist now so the mechanism is
//! exercised and tested before the vault ships. See
//! `docs/security/credential-vault-security-architecture.md` §15.5.

use crate::core::capture_exclusions;

/// Mark a SOURCE window title as never-capturable. Exact title match.
#[tauri::command]
pub fn register_capture_excluded_window(title: String) {
    capture_exclusions::register_excluded_window_title(&title);
}

#[tauri::command]
pub fn unregister_capture_excluded_window(title: String) {
    capture_exclusions::unregister_excluded_window_title(&title);
}

/// The frontend reports a sensitive surface becoming visible/hidden. While
/// any such surface is open, frame capture and keystroke recording into
/// SOURCE's own windows are suppressed (fail closed).
#[tauri::command]
pub fn sensitive_capture_surface_changed(open: bool) {
    if open {
        capture_exclusions::sensitive_surface_shown();
    } else {
        capture_exclusions::sensitive_surface_hidden();
    }
}

/// Whether capture is currently suppressed. Useful for status UI and tests.
#[tauri::command]
pub fn capture_suppression_active() -> bool {
    capture_exclusions::sensitive_surface_visible()
}
