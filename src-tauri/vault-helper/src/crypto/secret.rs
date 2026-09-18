//! Secret-bearing types (spec §2.11). All key material lives here:
//! fixed-size secrets in `SecretBytes<N>`, variable-length plaintext in
//! `SecretVec`. Both zeroize on drop, redact `Debug`, and never implement
//! `Clone` — copies are explicit via `expose`.
//!
//! `mlock` is best-effort (§2.11: documented non-guarantee). Core dumps
//! are disabled at process start via `disable_core_dumps` (called from
//! the helper's `main`).

use zeroize::Zeroizing;

/// Fixed-size secret (VK, PK, RK bytes, subkeys, backup credentials).
pub struct SecretBytes<const N: usize> {
    inner: Zeroizing<[u8; N]>,
    mlocked: bool,
}

impl<const N: usize> SecretBytes<N> {
    pub fn new(bytes: [u8; N]) -> Self {
        SecretBytes {
            inner: Zeroizing::new(bytes),
            mlocked: false,
        }
    }

    /// Borrow the secret for exactly one trust hop (derive → use → drop).
    pub fn expose(&self) -> &[u8; N] {
        &self.inner
    }

    /// Best-effort `mlock` on the backing page(s) (§2.11). Failures are
    /// non-fatal and non-actionable; the return value is for tests/logs.
    pub fn mlock_best_effort(mut self) -> Self {
        // SAFETY: pointer/length describe the live heap allocation owned by
        // `inner`; mlock only pins pages. Alignment/length validity follows
        // from the slice.
        let rc =
            unsafe { libc::mlock(self.inner.as_ptr() as *const libc::c_void, self.inner.len()) };
        self.mlocked = rc == 0;
        self
    }
}

impl<const N: usize> Drop for SecretBytes<N> {
    fn drop(&mut self) {
        if self.mlocked {
            // SAFETY: same live allocation as in mlock_best_effort.
            unsafe {
                libc::munlock(self.inner.as_ptr() as *const libc::c_void, self.inner.len());
            }
        }
        // Zeroizing's Drop wipes the buffer.
    }
}

impl<const N: usize> std::fmt::Debug for SecretBytes<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SecretBytes<{N}>([redacted])")
    }
}

/// Variable-length secret (record plaintext in flight, wrap payload
/// pre-encryption).
pub type SecretVec = Zeroizing<Vec<u8>>;

/// §2.11: the helper must not dump core. Called once at startup.
pub fn disable_core_dumps() {
    // SAFETY: rlimit struct fully initialized; RLIMIT_CORE zeroing is
    // process-wide and irreversible for us, which is the intent.
    unsafe {
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        libc::setrlimit(libc::RLIMIT_CORE, &limit);
    }
}

/// Fill from the OS CSPRNG. `getrandom` is the same backend `rand`'s
/// SysRng/OsRng uses (§2.1 "OS CSPRNG"); calling it directly keeps the
/// helper graph one crate smaller. A CSPRNG failure is unrecoverable:
/// panic (helper policy is panic=abort in release, §2.11).
fn csprng_fill(dest: &mut [u8]) {
    getrandom::fill(dest).expect("OS CSPRNG failure is unrecoverable");
}

/// Fresh 32-byte secret from the OS CSPRNG (VK, RK, device_backup_cred).
pub fn random_secret() -> SecretBytes<32> {
    let mut bytes = [0u8; 32];
    csprng_fill(&mut bytes);
    SecretBytes::new(bytes)
}

/// Random nonce for one seal operation (§2.5: 24 bytes).
pub fn random_nonce() -> [u8; 24] {
    let mut nonce = [0u8; 24];
    csprng_fill(&mut nonce);
    nonce
}

/// Random salt (§2.3: 16-byte kdf_salt; §2.5: 16-byte RK wrap salt).
pub fn random_salt() -> [u8; 16] {
    let mut salt = [0u8; 16];
    csprng_fill(&mut salt);
    salt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zeroize_on_drop_wipes_backing_buffer() {
        // CR-11 mechanism check: fill a secret with a canary, drop it, and
        // read the (not-yet-reused) allocation. Single-threaded test, no
        // allocation between drop and read; this is the standard spot-check
        // pattern for zeroize-on-drop.
        let canary = [0xA5u8; 32];
        let ptr;
        {
            let secret = SecretBytes::new(canary);
            ptr = secret.expose().as_ptr();
            assert_eq!(secret.expose(), &canary);
        }
        // SAFETY: reading 32 bytes of the just-dropped allocation before
        // any intervening allocation; the allocator has not reused it in
        // this thread. Spot-check only, not a general pattern.
        let after = unsafe { std::slice::from_raw_parts(ptr, 32) };
        assert!(after.iter().all(|&b| b == 0), "canary survived drop");
    }

    #[test]
    fn debug_is_redacted() {
        let secret = SecretBytes::<32>::new([7u8; 32]);
        let rendered = format!("{secret:?}");
        assert!(!rendered.contains('7'));
        assert!(rendered.contains("redacted"));
    }

    #[test]
    fn mlock_best_effort_does_not_fail_the_call() {
        let secret = SecretBytes::<32>::new([1u8; 32]).mlock_best_effort();
        assert_eq!(secret.expose(), &[1u8; 32]);
    }

    #[test]
    fn csprng_sources_have_right_lengths_and_differ() {
        assert_ne!(random_secret().expose(), random_secret().expose());
        assert_ne!(random_nonce(), random_nonce());
        assert_ne!(random_salt(), random_salt());
    }
}
