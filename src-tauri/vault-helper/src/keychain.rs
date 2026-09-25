//! Keychain items (spec §2.8): helper runtime bookkeeping and non-secret
//! prefs. Hand-rolled Security.framework FFI like `ffi/security.rs` —
//! no extra crate (§17.4 minimal-dependency rule).
//!
//! The Keychain never stores VK, wrap payloads, MP, RK, or backup
//! credentials; the two items here are exactly the §2.8 generic-password
//! rows. `WhenUnlockedThisDeviceOnly` accessibility; no ACL prompt is
//! involved for own-app generic passwords.

use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::data::CFData;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use std::ffi::c_void;

use crate::errors::ErrorCode;

pub type OSStatus = i32;
const ERR_SEC_SUCCESS: OSStatus = 0;
const ERR_SEC_ITEM_NOT_FOUND: OSStatus = -25300;
const ERR_SEC_DUPLICATE_ITEM: OSStatus = -25299;

type Query = CFDictionary<CFString, CFType>;

#[link(name = "Security", kind = "framework")]
extern "C" {
    fn SecItemCopyMatching(query: *const c_void, result: *mut CFTypeRef) -> OSStatus;
    fn SecItemAdd(attributes: *const c_void, result: *mut CFTypeRef) -> OSStatus;
    fn SecItemUpdate(query: *const c_void, attributes_to_update: *const c_void) -> OSStatus;
    fn SecItemDelete(query: *const c_void) -> OSStatus;
    #[cfg(debug_assertions)]
    fn SecKeychainSetUserInteractionAllowed(state: u8) -> OSStatus;
    static kSecClass: CFStringRef;
    static kSecClassGenericPassword: CFStringRef;
    static kSecAttrService: CFStringRef;
    static kSecAttrAccount: CFStringRef;
    static kSecAttrAccessible: CFStringRef;
    static kSecAttrAccessibleWhenUnlockedThisDeviceOnly: CFStringRef;
    static kSecValueData: CFStringRef;
    static kSecReturnData: CFStringRef;
    static kSecMatchLimit: CFStringRef;
    static kSecMatchLimitOne: CFStringRef;
}

/// Debug-only service-name prefix override so gate/test runs never touch
/// the user's real keychain items. Compiled out in release.
fn service(base: &str) -> String {
    #[cfg(debug_assertions)]
    if let Ok(prefix) = std::env::var("OV0_VAULT_KEYCHAIN_PREFIX") {
        if !prefix.is_empty() {
            return format!("{prefix}{base}");
        }
    }
    base.to_string()
}

const STATE_SERVICE: &str = "com.racker.zero.vault.state";
const PREFS_SERVICE: &str = "com.racker.zero.vault.helper-prefs";
const ACCOUNT: &str = "default";

/// Framework-owned string constant → CFString (Get rule). Reading the
/// extern static and borrowing it are both safe here: these are immutable
/// framework constants valid for the process lifetime.
macro_rules! k {
    ($name:ident) => {{
        let raw = unsafe { $name };
        unsafe { CFString::wrap_under_get_rule(raw) }
    }};
}

/// class=generic-password + service + account (the item's primary key).
fn base_pairs(service_name: &str) -> [(CFString, CFType); 3] {
    [
        (k!(kSecClass), k!(kSecClassGenericPassword).as_CFType()),
        (k!(kSecAttrService), CFString::new(service_name).as_CFType()),
        (k!(kSecAttrAccount), CFString::new(ACCOUNT).as_CFType()),
    ]
}

/// Read one item's data blob. `Ok(None)` = not found.
pub fn read_item(service_base: &str) -> Result<Option<Vec<u8>>, ErrorCode> {
    let service_name = service(service_base);
    let [p0, p1, p2] = base_pairs(&service_name);
    let query: Query = CFDictionary::from_CFType_pairs(&[
        p0,
        p1,
        p2,
        (k!(kSecReturnData), CFBoolean::true_value().as_CFType()),
        (k!(kSecMatchLimit), k!(kSecMatchLimitOne).as_CFType()),
    ]);
    let mut result: CFTypeRef = std::ptr::null();
    // SAFETY: `query` is a valid CFDictionary; `result` is a valid
    // out-pointer. On success we own the returned CFData (Copy rule).
    let status = unsafe { SecItemCopyMatching(query.as_concrete_TypeRef().cast(), &mut result) };
    match status {
        ERR_SEC_SUCCESS => {
            if result.is_null() {
                return Ok(None);
            }
            // SAFETY: result is a CFData we own (Copy rule).
            let data = unsafe { CFData::wrap_under_create_rule(result.cast()) };
            Ok(Some(data.bytes().to_vec()))
        }
        ERR_SEC_ITEM_NOT_FOUND => Ok(None),
        _ => Err(ErrorCode::Internal),
    }
}

/// Insert or replace one item's data blob.
pub fn upsert_item(service_base: &str, bytes: &[u8]) -> Result<(), ErrorCode> {
    let service_name = service(service_base);
    let data = CFData::from_buffer(bytes);
    let [p0, p1, p2] = base_pairs(&service_name);
    let attrs: Query = CFDictionary::from_CFType_pairs(&[
        p0,
        p1,
        p2,
        (
            k!(kSecAttrAccessible),
            k!(kSecAttrAccessibleWhenUnlockedThisDeviceOnly).as_CFType(),
        ),
        (k!(kSecValueData), data.as_CFType()),
    ]);
    // SAFETY: `attrs` is a valid CFDictionary; null out-param = no result
    // requested, so no object ownership is transferred.
    let status = unsafe { SecItemAdd(attrs.as_concrete_TypeRef().cast(), std::ptr::null_mut()) };
    if status == ERR_SEC_SUCCESS {
        return Ok(());
    }
    if status != ERR_SEC_DUPLICATE_ITEM {
        return Err(ErrorCode::Internal);
    }
    let [p0, p1, p2] = base_pairs(&service_name);
    let query: Query = CFDictionary::from_CFType_pairs(&[p0, p1, p2]);
    let update: Query =
        CFDictionary::from_CFType_pairs(&[(k!(kSecValueData), data.as_CFType())]);
    // SAFETY: both are valid CFDictionaries; update returns no object.
    let status = unsafe {
        SecItemUpdate(
            query.as_concrete_TypeRef().cast(),
            update.as_concrete_TypeRef().cast(),
        )
    };
    if status == ERR_SEC_SUCCESS {
        Ok(())
    } else {
        Err(ErrorCode::Internal)
    }
}

/// Test/gate fail-fast guard: disable interactive Keychain prompts for this
/// process, so an item whose ACL trusts another binary fails
/// (`errSecAuthFailed`) instead of raising a login-password dialog that
/// would hang an unattended run. Debug builds only; production ACLs and
/// behaviour are untouched.
#[cfg(debug_assertions)]
pub fn disable_user_interaction() {
    // SAFETY: plain FFI call with a boolean argument; no pointers involved.
    unsafe {
        SecKeychainSetUserInteractionAllowed(0);
    }
}

/// Remove one item (test/gate cleanup; never called in production paths).
#[cfg(debug_assertions)]
pub fn delete_item(service_base: &str) {
    let service_name = service(service_base);
    let [p0, p1, p2] = base_pairs(&service_name);
    let query: Query = CFDictionary::from_CFType_pairs(&[p0, p1, p2]);
    // SAFETY: `query` is a valid CFDictionary; delete returns no object.
    unsafe {
        SecItemDelete(query.as_concrete_TypeRef().cast());
    }
}

// --- Typed accessors for the two §2.8 items --------------------------------

/// Last manifest generation the helper has seen (rollback evidence).
/// `Ok(None)` = never recorded (pre-Phase-C vault or fresh machine).
pub fn read_seen_generation() -> Result<Option<u64>, ErrorCode> {
    let Some(bytes) = read_item(STATE_SERVICE)? else {
        return Ok(None);
    };
    let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| ErrorCode::Internal)?;
    Ok(v.get("manifest_generation").and_then(|g| g.as_u64()))
}

pub fn write_seen_generation(generation: u64) -> Result<(), ErrorCode> {
    let body = serde_json::json!({"manifest_generation": generation});
    upsert_item(STATE_SERVICE, body.to_string().as_bytes())
}

const DEFAULT_AUTO_LOCK_MINUTES: u32 = 15;
pub const AUTO_LOCK_MIN: u32 = 5;
pub const AUTO_LOCK_MAX: u32 = 60;

pub fn read_auto_lock_minutes() -> u32 {
    let minutes = read_item(PREFS_SERVICE)
        .ok()
        .flatten()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v.get("auto_lock_minutes").and_then(|m| m.as_u64()))
        .map(|m| m as u32);
    match minutes {
        Some(m) if (AUTO_LOCK_MIN..=AUTO_LOCK_MAX).contains(&m) => m,
        _ => DEFAULT_AUTO_LOCK_MINUTES,
    }
}

pub fn write_auto_lock_minutes(minutes: u32) -> Result<(), ErrorCode> {
    if !(AUTO_LOCK_MIN..=AUTO_LOCK_MAX).contains(&minutes) {
        return Err(ErrorCode::InvalidInput);
    }
    let body = serde_json::json!({"auto_lock_minutes": minutes});
    upsert_item(PREFS_SERVICE, body.to_string().as_bytes())
}
