//! Cooperative app activation (macOS 14+).
//!
//! The vault helper's secure panel (spec §1.7) must take keyboard focus the
//! moment it appears, so the user types into it directly and its secure
//! field turns on Secure Event Input. Since macOS 14 an app cannot make
//! itself active on its own: the currently active app has to yield first.
//! SOURCE is the active app when the user clicks Set up / Unlock / Change
//! master password, so it yields to the helper's bundle right before each
//! panel-presenting op; the helper's own `activate()` then succeeds.

/// Bundle identifier of SourceVaultHelper.app (scripts/build-helper.sh).
pub const VAULT_HELPER_BUNDLE_ID: &str = "com.racker.zero.vault-helper";

/// Yield activation to `bundle_id`. Main thread only (AppKit). No-op on
/// OS versions without the API and on non-macOS platforms.
#[cfg(target_os = "macos")]
pub fn yield_activation_to(bundle_id: &str) {
    use cocoa::base::{id, nil, BOOL, YES};
    use cocoa::foundation::NSString;
    use objc::{class, msg_send, sel, sel_impl};

    // SAFETY: plain AppKit messages on the shared application from the
    // main thread; the selector is checked before it is sent.
    unsafe {
        let app: id = msg_send![class!(NSApplication), sharedApplication];
        let sel = sel!(yieldActivationToApplicationWithBundleIdentifier:);
        let supported: BOOL = msg_send![app, respondsToSelector: sel];
        if supported != YES {
            return;
        }
        let ns_id = NSString::alloc(nil).init_str(bundle_id);
        let _: () = msg_send![app, yieldActivationToApplicationWithBundleIdentifier: ns_id];
        let _: () = msg_send![ns_id, release];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn yield_activation_to(_bundle_id: &str) {}

/// Bundle identifier of the frontmost application, if any. Main or any
/// thread (NSWorkspace is thread-safe for this read).
#[cfg(target_os = "macos")]
pub fn frontmost_bundle_id() -> Option<String> {
    use cocoa::base::{id, nil};
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CStr;

    // SAFETY: read-only NSWorkspace/NSRunningApplication/NSString queries;
    // every pointer is nil-checked before use.
    unsafe {
        let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let app: id = msg_send![workspace, frontmostApplication];
        if app == nil {
            return None;
        }
        let bundle: id = msg_send![app, bundleIdentifier];
        if bundle == nil {
            return None;
        }
        let c_str: *const std::os::raw::c_char = msg_send![bundle, UTF8String];
        (!c_str.is_null()).then(|| CStr::from_ptr(c_str).to_string_lossy().into_owned())
    }
}

#[cfg(not(target_os = "macos"))]
pub fn frontmost_bundle_id() -> Option<String> {
    None
}
