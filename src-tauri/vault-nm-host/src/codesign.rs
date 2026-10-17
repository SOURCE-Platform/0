//! The vault helper's code identity, checked before nm-host sends it
//! anything (spec §1.4 item 4): same user, then the peer — resolved by its
//! audit token, never a bare pid (final review SEC-I1) — must satisfy the
//! helper's designated requirement. System frameworks only, no crate.

use std::ffi::c_void;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;

type CFRef = *const c_void;

const PROD_TEAM_OU: &str = "9RGW34CMA2";
const HELPER_ID: &str = "com.racker.zero.vault-helper";
const UTF8: u32 = 0x0800_0100;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithBytes(alloc: CFRef, bytes: *const u8, len: isize, encoding: u32, external: u8) -> CFRef;
    fn CFDataCreate(alloc: CFRef, bytes: *const u8, len: isize) -> CFRef;
    fn CFDictionaryCreate(alloc: CFRef, keys: *const CFRef, values: *const CFRef, n: isize, kcb: CFRef, vcb: CFRef) -> CFRef;
    fn CFRelease(cf: CFRef);
    static kCFTypeDictionaryKeyCallBacks: [usize; 6];
    static kCFTypeDictionaryValueCallBacks: [usize; 5];
}

#[link(name = "Security", kind = "framework")]
extern "C" {
    fn SecRequirementCreateWithString(text: CFRef, flags: u32, req: *mut CFRef) -> i32;
    fn SecCodeCopyGuestWithAttributes(host: CFRef, attrs: CFRef, flags: u32, guest: *mut CFRef) -> i32;
    fn SecCodeCheckValidityWithErrors(code: CFRef, flags: u32, req: CFRef, errors: *mut CFRef) -> i32;
    static kSecGuestAttributeAudit: CFRef;
}

extern "C" {
    fn getpeereid(fd: i32, uid: *mut u32, gid: *mut u32) -> i32;
    fn geteuid() -> u32;
    fn getsockopt(fd: i32, level: i32, name: i32, value: *mut c_void, len: *mut u32) -> i32;
}

/// Released on drop (every `Create`/`Copy` result we own).
struct Owned(CFRef);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a CF object this module created and owns.
            unsafe { CFRelease(self.0) };
        }
    }
}

/// The team OU in the requirement: the production team, or in a debug
/// build the development team the build script compiled in.
fn team_ou() -> &'static str {
    #[cfg(debug_assertions)]
    if let Some(ou) = option_env!("OV0_VAULT_DEV_TEAM_OU").filter(|o| !o.is_empty()) {
        return ou;
    }
    PROD_TEAM_OU
}

pub fn helper_requirement() -> String {
    format!("anchor apple generic and certificate leaf[subject.OU] = \"{}\" and identifier \"{HELPER_ID}\"", team_ou())
}

fn audit_token(stream: &UnixStream) -> Option<[u8; 32]> {
    const SOL_LOCAL: i32 = 0;
    const LOCAL_PEERTOKEN: i32 = 0x006;
    let mut token = [0u8; 32];
    let mut len = token.len() as u32;
    // SAFETY: a live fd; `token`/`len` sized as the option writes.
    let rc = unsafe { getsockopt(stream.as_raw_fd(), SOL_LOCAL, LOCAL_PEERTOKEN, token.as_mut_ptr() as *mut c_void, &mut len) };
    (rc == 0 && len as usize == token.len()).then_some(token)
}

/// True only when the peer runs as this user and is the signed helper.
pub fn peer_is_helper(stream: &UnixStream) -> bool {
    let (mut uid, mut gid) = (0u32, 0u32);
    // SAFETY: a live fd and valid out-pointers.
    if unsafe { getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 || uid != unsafe { geteuid() } {
        return false;
    }
    let Some(token) = audit_token(stream) else { return false };
    let text = helper_requirement();
    // SAFETY: every pointer below is either null-checked or a live CF
    // object owned by an `Owned`; the framework constants are borrowed.
    unsafe {
        let s = Owned(CFStringCreateWithBytes(std::ptr::null(), text.as_ptr(), text.len() as isize, UTF8, 0));
        let mut req: CFRef = std::ptr::null();
        if s.0.is_null() || SecRequirementCreateWithString(s.0, 0, &mut req) != 0 {
            return false;
        }
        let req = Owned(req);
        let data = Owned(CFDataCreate(std::ptr::null(), token.as_ptr(), token.len() as isize));
        let keys = [kSecGuestAttributeAudit];
        let values = [data.0];
        let attrs = Owned(CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks as *const _ as CFRef,
            &kCFTypeDictionaryValueCallBacks as *const _ as CFRef,
        ));
        let mut guest: CFRef = std::ptr::null();
        if data.0.is_null() || attrs.0.is_null() || SecCodeCopyGuestWithAttributes(std::ptr::null(), attrs.0, 0, &mut guest) != 0 {
            return false;
        }
        let guest = Owned(guest);
        !guest.0.is_null() && SecCodeCheckValidityWithErrors(guest.0, 0, req.0, std::ptr::null_mut()) == 0
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_unsigned_peer_is_not_the_helper() {
        // The test binary is ad-hoc signed: never `anchor apple generic`.
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        assert!(!super::peer_is_helper(&a));
        assert!(super::helper_requirement().contains("com.racker.zero.vault-helper"));
    }
}
