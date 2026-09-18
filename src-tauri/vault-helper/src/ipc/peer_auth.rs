//! Bidirectional SecCode peer authentication (spec §1.4).
//!
//! Both directions run the same shape: `getpeereid` UID equality as a cheap
//! first gate (spec §1.4 item 5), then the peer pid via `LOCAL_PEEREPID`,
//! then `SecCodeCopyGuestWithAttributes` + `SecCodeCheckValidityWithErrors`
//! against a pinned designated requirement:
//!
//! - helper verifying a client: app/nm-host requirement;
//! - client verifying the helper: helper requirement.
//!
//! The requirement pins the Apple anchor, the team OU, and the exact bundle
//! identifiers. Debug builds may substitute a development team OU via the
//! `OV0_VAULT_DEV_TEAM_OU` *compile-time* env var (signing strategy doc rule
//! 3: relax to same-team, never to "any process"). Release builds always use
//! `PROD_TEAM_OU`; the env var is ignored there.

use std::io;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;

use crate::ffi::security as sec;

/// Production team identifier pinned by the spec (§1.4) and the signing
/// strategy doc.
pub const PROD_TEAM_OU: &str = "9RGW34CMA2";

pub const APP_BUNDLE_ID: &str = "com.racker.zero";
pub const NM_HOST_BUNDLE_ID: &str = "com.racker.zero.nm-host";
pub const HELPER_BUNDLE_ID: &str = "com.racker.zero.vault-helper";

/// The team OU compiled into the designated requirements. In debug builds
/// with `OV0_VAULT_DEV_TEAM_OU` set at compile time this is the development
/// team; everywhere else it is `PROD_TEAM_OU`. The identifiers below are
/// pinned in both cases.
pub fn team_ou() -> &'static str {
    #[cfg(debug_assertions)]
    if let Some(ou) = option_env!("OV0_VAULT_DEV_TEAM_OU") {
        if !ou.is_empty() {
            return ou;
        }
    }
    PROD_TEAM_OU
}

/// Designated requirement a *client process* must satisfy when connecting
/// to the helper (spec §1.4 item 3).
pub fn client_requirement_string() -> String {
    format!(
        "anchor apple generic and certificate leaf[subject.OU] = \"{}\" \
         and (identifier \"{}\" or identifier \"{}\")",
        team_ou(),
        APP_BUNDLE_ID,
        NM_HOST_BUNDLE_ID
    )
}

/// Designated requirement the *helper process* must satisfy from the
/// client's point of view (spec §1.4 item 4).
pub fn helper_requirement_string() -> String {
    format!(
        "anchor apple generic and certificate leaf[subject.OU] = \"{}\" \
         and identifier \"{}\"",
        team_ou(),
        HELPER_BUNDLE_ID
    )
}

#[derive(Debug)]
pub enum AuthError {
    /// getpeereid failed or the peer runs under a different UID.
    EuidMismatch,
    /// Could not obtain the peer's pid from the socket.
    PidLookup(io::Error),
    /// The peer's code identity does not satisfy the requirement.
    /// Carries the Security.framework status for diagnostics (never
    /// secret-bearing).
    CodeInvalid(sec::OSStatus),
    /// The pinned requirement string itself failed to compile — a build or
    /// platform bug; fail closed.
    RequirementInvalid(sec::OSStatus),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::EuidMismatch => write!(f, "peer UID mismatch"),
            AuthError::PidLookup(e) => write!(f, "peer pid lookup failed: {e}"),
            AuthError::CodeInvalid(s) => write!(f, "peer code invalid (OSStatus {s})"),
            AuthError::RequirementInvalid(s) => {
                write!(f, "designated requirement failed to compile (OSStatus {s})")
            }
        }
    }
}

impl std::error::Error for AuthError {}

/// Cheap first gate (spec §1.4 item 5): the peer must run under our UID.
fn peer_uid_matches(stream: &UnixStream) -> Result<bool, AuthError> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: valid fd from a live UnixStream; uid/gid out-pointers valid.
    let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    if rc != 0 {
        // getpeereid failing is treated as an auth failure, not success.
        return Err(AuthError::EuidMismatch);
    }
    // SAFETY: geteuid is always safe.
    Ok(uid == unsafe { libc::geteuid() })
}

/// Peer pid via getsockopt(LOCAL_PEEREPID). LOCAL_PEEREPID is 0x002 on
/// Darwin (sys/un.h); a stable kernel ABI constant. SOL_LOCAL is 0.
fn peer_pid(stream: &UnixStream) -> Result<i32, AuthError> {
    const LOCAL_PEEREPID: libc::c_int = 0x002;
    const SOL_LOCAL: libc::c_int = 0;
    let mut pid: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: valid fd; `pid` and `len` are valid out-parameters sized
    // exactly as the option expects.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            SOL_LOCAL,
            LOCAL_PEEREPID,
            &mut pid as *mut libc::c_int as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 || pid <= 0 {
        return Err(AuthError::PidLookup(io::Error::last_os_error()));
    }
    Ok(pid)
}

fn verify(stream: &UnixStream, requirement_text: &str) -> Result<(), AuthError> {
    if !peer_uid_matches(stream)? {
        return Err(AuthError::EuidMismatch);
    }
    let pid = peer_pid(stream)?;
    let requirement =
        sec::requirement_from_string(requirement_text).map_err(AuthError::RequirementInvalid)?;
    sec::check_pid_against_requirement(pid, &requirement).map_err(AuthError::CodeInvalid)
}

/// Helper side: authenticate an accepted client connection (spec §1.4 item 3).
pub fn verify_client(stream: &UnixStream) -> Result<(), AuthError> {
    verify(stream, &client_requirement_string())
}

/// Client side: authenticate the helper after connect (spec §1.4 item 4).
pub fn verify_helper(stream: &UnixStream) -> Result<(), AuthError> {
    verify(stream, &helper_requirement_string())
}

/// Pre-launch static check (spec §1.4 item 1): the bundle on disk must
/// satisfy the helper requirement before LaunchServices starts it.
pub fn verify_helper_bundle(path: &std::path::Path) -> Result<(), AuthError> {
    let requirement = sec::requirement_from_string(&helper_requirement_string())
        .map_err(AuthError::RequirementInvalid)?;
    sec::check_path_against_requirement(path, &requirement).map_err(AuthError::CodeInvalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirement_strings_pin_anchor_ou_and_identifiers() {
        let client = client_requirement_string();
        assert!(client.contains("anchor apple generic"));
        assert!(client.contains(team_ou()));
        assert!(client.contains(APP_BUNDLE_ID));
        assert!(client.contains(NM_HOST_BUNDLE_ID));
        let helper = helper_requirement_string();
        assert!(helper.contains(HELPER_BUNDLE_ID));
        // The helper DR must never widen to "any process".
        assert!(!helper.contains("identifier *"));
    }

    #[test]
    fn requirements_compile_in_security_framework() {
        sec::requirement_from_string(&client_requirement_string()).expect("client DR must compile");
        sec::requirement_from_string(&helper_requirement_string()).expect("helper DR must compile");
    }

    #[test]
    fn socketpair_peer_lookup_matches_self() {
        let (a, _b) = UnixStream::pair().unwrap();
        assert!(peer_uid_matches(&a).unwrap());
        assert_eq!(peer_pid(&a).unwrap(), std::process::id() as i32);
    }

    #[test]
    fn unsigned_test_binary_fails_pinned_requirement() {
        // `cargo test` binaries are ad-hoc signed: they cannot satisfy
        // `anchor apple generic`, so verification MUST fail. This is the
        // unit-level analog of the Phase A gate's unsigned-clone rejection;
        // the full signed matrix lives in scripts/phase-a-gate.sh.
        let (a, _b) = UnixStream::pair().unwrap();
        let result = verify_client(&a);
        assert!(matches!(result, Err(AuthError::CodeInvalid(_))));
    }

    #[test]
    fn system_binary_fails_helper_bundle_requirement() {
        // /bin/ls is Apple-platform code, not our team+identifier.
        let result = verify_helper_bundle(std::path::Path::new("/bin/ls"));
        assert!(result.is_err());
    }
}
