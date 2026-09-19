//! Panel form helpers: field plans, validation, secret extraction, and
//! the debug-only UI-02 probe. Split from `appkit.rs` under the repo's
//! 350-line file cap; everything here is pure Rust apart from the one
//! Carbon read-only query in `write_probe`.

use std::ffi::CStr;

use objc2_app_kit::NSSecureTextField;
use zeroize::Zeroizing;

use crate::crypto::secret::SecretVec;
use crate::vault::PanelRequest;

const MIN_MP_CHARS: usize = 8;

// Carbon HIToolbox secure-event-input state (UI-02 probe, debug only).
#[cfg(debug_assertions)]
#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> bool;
}

pub(super) fn validate(mode: u8, values: &[Zeroizing<String>]) -> Result<(), &'static str> {
    match mode {
        1 | 2 => {
            // create: [new, confirm]; change: [old, new, confirm]
            let (new, confirm) = if mode == 1 {
                (&values[0], &values[1])
            } else {
                (&values[1], &values[2])
            };
            if new.chars().count() < MIN_MP_CHARS {
                return Err("Master password must be at least 8 characters.");
            }
            if new != confirm {
                return Err("Passwords do not match.");
            }
            Ok(())
        }
        3 => {
            // Word count only; the checksum is verified by the op (§2.4)
            // so a typo gets the same answer as a wrong key.
            if values.first().map_or(0, |v| v.split_whitespace().count()) != 24 {
                return Err("Enter all 24 words of your Recovery Key.");
            }
            Ok(())
        }
        _ => {
            if values.first().is_some_and(|v| v.is_empty()) {
                return Err("Enter your master password.");
            }
            Ok(())
        }
    }
}


pub(super) fn read_field(field: &NSSecureTextField) -> Zeroizing<String> {
    // SAFETY: UTF8String returns a valid NUL-terminated UTF-8 pointer
    // owned by the NSString; we copy out before the next autorelease turn.
    let ptr = unsafe { field.stringValue().UTF8String() };
    if ptr.is_null() {
        return Zeroizing::new(String::new());
    }
    Zeroizing::new(unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned())
}


pub(super) fn to_secret(s: Zeroizing<String>) -> SecretVec {
    SecretVec::new(s.as_bytes().to_vec())
}

pub(super) fn field_plan(req: &PanelRequest) -> (Vec<&'static str>, u8) {
    match req {
        PanelRequest::MpEntry => (vec!["Master password"], 0),
        PanelRequest::MpCreate => (vec!["New master password", "Confirm master password"], 1),
        PanelRequest::RkEntry => (vec!["Recovery Key — all 24 words, separated by spaces"], 3),
        PanelRequest::MpChange => (
            vec!["Current master password", "New master password", "Confirm new password"],
            2,
        ),
    }
}

/// UI-02 evidence on a live window: record that the focused field is a
/// native NSSecureTextField and that macOS reports secure event input.
/// Debug builds only, path from OV0_VAULT_PANEL_PROBE.
#[cfg(debug_assertions)]
pub(super) fn write_probe(title: &str, app_active: bool, panel_key: bool) {
    let Ok(path) = std::env::var("OV0_VAULT_PANEL_PROBE") else {
        return;
    };
    // SAFETY: Carbon query, read-only.
    let secure_input = unsafe { IsSecureEventInputEnabled() };
    let body = serde_json::json!({
        "title": title,
        "field_class": "NSSecureTextField",
        "secure_event_input_active": secure_input,
        "app_active": app_active,
        "panel_key": panel_key,
    });
    let _ = std::fs::write(path, body.to_string());
}

/// UI-04 evidence from the Recovery Key window's debug print path:
/// whether the print pipeline ran, and that the window (and with it the
/// capture-suppression bracket) was up during printing. Contains no
/// secret. Debug builds only, path from OV0_VAULT_PANEL_PROBE.
#[cfg(debug_assertions)]
pub(super) fn write_sheet_probe(print_attempted: bool, print_ran: bool, on_screen: bool, window_visible: bool) {
    let Ok(path) = std::env::var("OV0_VAULT_PANEL_PROBE") else {
        return;
    };
    let body = serde_json::json!({
        "print_attempted": print_attempted,
        "print_ran": print_ran,
        "sheet_on_screen": on_screen,
        "window_visible": window_visible,
        "sharing_type": "NSWindowSharingNone",
    });
    let _ = std::fs::write(path, body.to_string());
}
