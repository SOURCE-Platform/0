//! Argon2id master-password KDF (spec §2.3, RFC 9106).
//!
//! v1 parameters: m = 64 MiB, t = 3, p = 1, 32-byte output, 16-byte random
//! salt — the RFC 9106 §7.4 second profile's memory and iteration cost
//! with application-specific single-lane parallelism (see the spec's
//! corrected attribution and `docs/security/argon2-calibration.md` for the
//! Phase B benchmark evidence behind this tuple).

use argon2::{Algorithm, Argon2, Params, Version};
use serde::{Deserialize, Serialize};

use super::secret::SecretBytes;
use super::CryptoError;

pub const KDF_VERSION_V1: u32 = 1;
pub const PK_LEN: usize = 32;
pub const SALT_LEN: usize = 16;

pub const V1_M_KIB: u32 = 64 * 1024; // 64 MiB
pub const V1_T: u32 = 3;
pub const V1_P: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argon2Params {
    /// Memory cost in KiB (65536 = 64 MiB).
    pub m: u32,
    pub t: u32,
    pub p: u32,
}

impl Argon2Params {
    pub const V1: Argon2Params = Argon2Params {
        m: V1_M_KIB,
        t: V1_T,
        p: V1_P,
    };

    fn to_argon2(self) -> Result<Argon2<'static>, CryptoError> {
        let params = Params::new(self.m, self.t, self.p, Some(PK_LEN))
            .map_err(|_| CryptoError::KdfParams)?;
        Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
    }
}

/// PK = Argon2id(MP, kdf_salt, params) — 32 bytes, transient (§2.2).
/// `password` bytes are the caller's zeroized buffer; the helper never
/// retains MP or PK beyond the call that used them (§2.11).
pub fn derive_pk(
    password: &[u8],
    salt: &[u8; SALT_LEN],
    params: Argon2Params,
) -> Result<SecretBytes<PK_LEN>, CryptoError> {
    let argon2 = params.to_argon2()?;
    let mut out = [0u8; PK_LEN];
    argon2
        .hash_password_into(password, salt, &mut out)
        .map_err(|_| CryptoError::KdfParams)?;
    Ok(SecretBytes::new(out))
}

/// §2.3 downgrade rule (CR-09): the helper refuses a `kdf_version` lower
/// than the highest it has ever seen for this vault.
pub fn check_no_kdf_downgrade(header_version: u32, highest_seen: u32) -> Result<(), CryptoError> {
    if header_version < highest_seen {
        return Err(CryptoError::KdfDowngrade);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fast parameters for logic tests; the real v1 tuple is exercised by
    /// CR-01 and the calibration bench.
    const FAST: Argon2Params = Argon2Params {
        m: 8 * 1024,
        t: 1,
        p: 1,
    };

    #[test]
    fn derive_is_deterministic_and_salt_bound() {
        let salt = [1u8; SALT_LEN];
        let a = derive_pk(b"correct horse battery staple", &salt, FAST).unwrap();
        let b = derive_pk(b"correct horse battery staple", &salt, FAST).unwrap();
        assert_eq!(a.expose(), b.expose());
        let c = derive_pk(b"correct horse battery staple", &[2u8; SALT_LEN], FAST).unwrap();
        assert_ne!(a.expose(), c.expose());
        let d = derive_pk(b"Correct horse battery staple", &salt, FAST).unwrap();
        assert_ne!(a.expose(), d.expose());
    }

    #[test]
    fn params_roundtrip_serde() {
        let json = serde_json::to_string(&Argon2Params::V1).unwrap();
        assert_eq!(json, r#"{"m":65536,"t":3,"p":1}"#);
        let back: Argon2Params = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Argon2Params::V1);
    }

    #[test]
    fn downgrade_refused() {
        assert!(check_no_kdf_downgrade(1, 1).is_ok());
        assert!(check_no_kdf_downgrade(2, 1).is_ok());
        assert_eq!(check_no_kdf_downgrade(1, 2), Err(CryptoError::KdfDowngrade));
    }

    #[test]
    fn v1_tuple_derivation_runs() {
        // One full-cost derivation to prove the production path; the
        // calibration sweep lives in src/bin/kdf_bench.rs.
        let salt = [9u8; SALT_LEN];
        let pk = derive_pk(b"bench", &salt, Argon2Params::V1).unwrap();
        assert_eq!(pk.expose().len(), PK_LEN);
    }
}
