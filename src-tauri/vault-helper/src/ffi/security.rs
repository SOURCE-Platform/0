//! Minimal Security.framework bindings for code-identity checks
//! (spec §1.4, signing strategy doc §"The future vault helper" rule 2).
//!
//! Only the six entry points the helper needs are declared — no broad
//! `security-framework` crate dependency (spec §17.4 minimal-dependency
//! rule for the helper). Each `unsafe` call site documents its invariants;
//! this file is on the §17.4 focused-review list.

use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation::url::CFURL;
use std::ffi::c_void;
use std::path::Path;

pub type OSStatus = i32;
type SecRequirementRef = *mut c_void;
type SecCodeRef = *mut c_void;
type SecStaticCodeRef = *mut c_void;

const ERR_SEC_SUCCESS: OSStatus = 0;
/// kSecCSDefaultFlags — no optional validity checks toggled.
const SEC_CS_DEFAULT_FLAGS: u32 = 0;

#[link(name = "Security", kind = "framework")]
extern "C" {
    fn SecRequirementCreateWithString(
        text: core_foundation::string::CFStringRef,
        flags: u32,
        requirement: *mut SecRequirementRef,
    ) -> OSStatus;
    fn SecCodeCopyGuestWithAttributes(
        host: SecCodeRef,
        attributes: core_foundation::dictionary::CFDictionaryRef,
        flags: u32,
        guest: *mut SecCodeRef,
    ) -> OSStatus;
    fn SecCodeCheckValidityWithErrors(
        code: SecCodeRef,
        flags: u32,
        requirement: SecRequirementRef,
        errors: CFTypeRef,
    ) -> OSStatus;
    fn SecStaticCodeCreateWithPath(
        path: core_foundation::url::CFURLRef,
        flags: u32,
        static_code: *mut SecStaticCodeRef,
    ) -> OSStatus;
    fn SecStaticCodeCheckValidityWithErrors(
        static_code: SecStaticCodeRef,
        flags: u32,
        requirement: SecRequirementRef,
        errors: CFTypeRef,
    ) -> OSStatus;
    /// kSecGuestAttributePid — attribute key selecting a guest by pid.
    static kSecGuestAttributePid: core_foundation::string::CFStringRef;
}

/// Owned SecRequirement; released on drop.
pub struct SecRequirement(SecRequirementRef);

impl Drop for SecRequirement {
    fn drop(&mut self) {
        // SAFETY: self.0 is a live CF-style object we own (Create rule).
        unsafe { CFRelease(self.0 as CFTypeRef) };
    }
}

/// Compile a designated-requirement string.
pub fn requirement_from_string(text: &str) -> Result<SecRequirement, OSStatus> {
    let text = CFString::new(text);
    let mut req: SecRequirementRef = std::ptr::null_mut();
    // SAFETY: `text` is a valid CFString; `req` is a valid out-pointer.
    // On success we own `req` (Create rule) and wrap it in SecRequirement.
    let status = unsafe {
        SecRequirementCreateWithString(text.as_concrete_TypeRef(), SEC_CS_DEFAULT_FLAGS, &mut req)
    };
    if status == ERR_SEC_SUCCESS && !req.is_null() {
        Ok(SecRequirement(req))
    } else {
        Err(status)
    }
}

/// Map a pid to its dynamic SecCode and check it against `requirement`.
/// This is the §1.4 client-side check (helper verifying a peer process)
/// and, with the helper requirement, the reverse check.
pub fn check_pid_against_requirement(
    pid: i32,
    requirement: &SecRequirement,
) -> Result<(), OSStatus> {
    // SAFETY: kSecGuestAttributePid is a framework-owned constant;
    // wrap_under_get_rule borrows without retaining (Get rule).
    let key = unsafe { CFString::wrap_under_get_rule(kSecGuestAttributePid) };
    let value = CFNumber::from(pid);
    let attrs = CFDictionary::from_CFType_pairs(&[(key, value)]);
    let mut guest: SecCodeRef = std::ptr::null_mut();
    // SAFETY: null host = this host; `attrs` is a valid CFDictionary with
    // the documented pid key; `guest` is a valid out-pointer. On success we
    // own `guest` (Copy rule) and release it after the validity check.
    let status = unsafe {
        SecCodeCopyGuestWithAttributes(
            std::ptr::null_mut(),
            attrs.as_concrete_TypeRef(),
            SEC_CS_DEFAULT_FLAGS,
            &mut guest,
        )
    };
    if status != ERR_SEC_SUCCESS || guest.is_null() {
        return Err(status);
    }
    // SAFETY: `guest` and `requirement.0` are live objects; errors out-param
    // is null (we only need the status code, never secret-bearing detail).
    let check = unsafe {
        SecCodeCheckValidityWithErrors(guest, SEC_CS_DEFAULT_FLAGS, requirement.0, std::ptr::null())
    };
    // SAFETY: we own `guest` per the Copy rule.
    unsafe { CFRelease(guest as CFTypeRef) };
    if check == ERR_SEC_SUCCESS {
        Ok(())
    } else {
        Err(check)
    }
}

/// Pre-launch static check (spec §1.4 item 1): verify the code on disk at
/// `path` against `requirement` without starting it.
pub fn check_path_against_requirement(
    path: &Path,
    requirement: &SecRequirement,
) -> Result<(), OSStatus> {
    let Some(url) = CFURL::from_path(path, true) else {
        return Err(-50); // paramErr: unrepresentable path
    };
    let mut code: SecStaticCodeRef = std::ptr::null_mut();
    // SAFETY: `url` is a valid CFURL; `code` is a valid out-pointer. On
    // success we own `code` (Copy/Create rule) and release it below.
    let status = unsafe {
        SecStaticCodeCreateWithPath(url.as_concrete_TypeRef(), SEC_CS_DEFAULT_FLAGS, &mut code)
    };
    if status != ERR_SEC_SUCCESS || code.is_null() {
        return Err(status);
    }
    // SAFETY: both objects live; null errors out-param as above.
    let check = unsafe {
        SecStaticCodeCheckValidityWithErrors(
            code,
            SEC_CS_DEFAULT_FLAGS,
            requirement.0,
            std::ptr::null(),
        )
    };
    // SAFETY: we own `code`.
    unsafe { CFRelease(code as CFTypeRef) };
    if check == ERR_SEC_SUCCESS {
        Ok(())
    } else {
        Err(check)
    }
}
