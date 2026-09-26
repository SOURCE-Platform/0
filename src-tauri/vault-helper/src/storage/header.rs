//! `header.json` v2 (spec v0.4 §2.10). The model lives in `vault-proto`
//! (the provider parses it too); this module adds the helper's own
//! policy: the §2.3 KDF allowlist on every local parse, and the §11.4
//! compiled-in provider-origin allowlist.

pub use vault_proto::header::{
    check_kdf_policy, strict_hex, write_header, Header, Hex16, Hex32, KdfBlock, HEADER_VERSION,
};

use crate::crypto::kdf::Argon2Params;
use crate::errors::ErrorCode;

/// Provider origins this build will sign for (§11.4). v1 release: exactly
/// one production origin — an owner decision not yet made, so a release
/// build has none and refuses to set up or recover a vault until it is
/// compiled in. Debug builds add the test origins.
const RELEASE_ORIGINS: &[&str] = &[];
#[cfg(debug_assertions)]
const DEBUG_ORIGINS: &[&str] = &["https://provider.test", "http://127.0.0.1:8787"];
#[cfg(not(debug_assertions))]
const DEBUG_ORIGINS: &[&str] = &[];

pub fn provider_allowed(origin: &str) -> bool {
    RELEASE_ORIGINS.contains(&origin) || DEBUG_ORIGINS.contains(&origin)
}

/// The origin a new vault is set up against: the first allowlisted one.
pub fn default_provider() -> Result<&'static str, ErrorCode> {
    RELEASE_ORIGINS.iter().chain(DEBUG_ORIGINS).next().copied().ok_or(ErrorCode::BackupUnavailable)
}

/// §2.3/§11.4: refuse a header naming an origin outside the allowlist.
pub fn check_provider(origin: &str) -> Result<(), ErrorCode> {
    if provider_allowed(origin) {
        Ok(())
    } else {
        Err(ErrorCode::SigningRefused)
    }
}

/// The Argon2 parameters of an (allowlisted) header KDF block.
pub fn kdf_params(k: &KdfBlock) -> Argon2Params {
    Argon2Params { m: k.m_kib, t: k.t, p: k.p }
}

/// §3.5 local parse: structure, then the §2.3 KDF allowlist (a local
/// header outside it is never used to derive anything).
pub fn parse_header(bytes: &[u8]) -> Result<Header, ErrorCode> {
    let h = vault_proto::header::parse_header(bytes)?;
    check_kdf_policy(&h.kdf)?;
    Ok(h)
}

/// A fresh v2 header against the default provider origin.
pub fn fresh_header(vault_id: Hex16) -> Result<Header, ErrorCode> {
    Ok(Header::fresh(vault_id, default_provider()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h() -> Header {
        fresh_header(Hex16::random()).unwrap()
    }

    #[test]
    fn round_trip_header() {
        let h = h();
        let parsed = parse_header(&write_header(&h).unwrap()).unwrap();
        assert_eq!(parsed, h);
        assert_eq!(parsed.version, 2);
        assert_eq!(kdf_params(&parsed.kdf), Argon2Params::V1);
    }

    #[test]
    fn versions_fail_closed() {
        for (v, want) in [(1u32, ErrorCode::FormatInvalid), (0, ErrorCode::FormatInvalid), (3, ErrorCode::FormatTooNew)] {
            let mut x = h();
            x.version = v;
            assert_eq!(parse_header(&write_header(&x).unwrap()), Err(want), "v={v}");
        }
    }

    #[test]
    fn corrupt_or_unknown_fields_refused() {
        assert_eq!(parse_header(b"{not json"), Err(ErrorCode::FormatInvalid));
        let mut v: serde_json::Value = serde_json::from_slice(&write_header(&h()).unwrap()).unwrap();
        v["locator_salt_mp"] = serde_json::json!("00".repeat(16));
        assert_eq!(parse_header(v.to_string().as_bytes()), Err(ErrorCode::FormatInvalid), "retired v0.3 field");
    }

    /// SC-03: every salt is exactly 16 bytes of lowercase hex.
    #[test]
    fn salt_lengths_enforced() {
        for field in ["import_fp_salt", "auth_salt_mp", "auth_salt_rk", "meta_salt"] {
            let mut v: serde_json::Value = serde_json::from_slice(&write_header(&h()).unwrap()).unwrap();
            v[field] = serde_json::json!("aabbcc");
            assert_eq!(parse_header(v.to_string().as_bytes()), Err(ErrorCode::FormatInvalid), "{field}");
            let mut v: serde_json::Value = serde_json::from_slice(&write_header(&h()).unwrap()).unwrap();
            v[field] = serde_json::json!("AA".repeat(16));
            assert_eq!(parse_header(v.to_string().as_bytes()), Err(ErrorCode::FormatInvalid), "{field} uppercase");
        }
    }

    /// §2.3: any tuple outside the allowlist — weaker, stronger, unknown.
    #[test]
    fn kdf_outside_allowlist_refused() {
        let tweaks: [fn(&mut KdfBlock); 6] = [
            |k| k.m_kib = 32768,
            |k| k.m_kib = 131072,
            |k| k.t = 2,
            |k| k.p = 4,
            |k| k.version = 0x10,
            |k| k.alg = "scrypt".into(),
        ];
        for t in tweaks {
            let mut x = h();
            t(&mut x.kdf);
            assert_eq!(parse_header(&write_header(&x).unwrap()), Err(ErrorCode::KdfPolicyViolation));
        }
    }

    #[test]
    fn provider_allowlist() {
        assert!(provider_allowed("https://provider.test"));
        assert!(!provider_allowed("https://evil.test"));
        assert_eq!(check_provider("https://evil.test"), Err(ErrorCode::SigningRefused));
    }
}
