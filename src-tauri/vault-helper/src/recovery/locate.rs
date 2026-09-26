//! The unauthenticated `POST /v2/recover/locate` response (spec v0.4
//! §11.5), checked before any MP prompt or derivation:
//!
//! 1. strict parse — exactly the four keys (and the seven `kdf` keys),
//!    lowercase hex, `vault_id` and every salt exactly 16 bytes;
//! 2. `kdf` must equal an allowlisted tuple exactly — weaker, stronger,
//!    unknown or malformed → `KDF_POLICY_VIOLATION`, recovery stops.
//!
//! The helper then derives with its compiled-in parameters, never the
//! provider's. After authentication the committed header must byte-equal
//! these fields (`cross_check`, else `RECOVERY_METADATA_MISMATCH`).

use serde_json::Value;

use crate::errors::ErrorCode;
use crate::storage::header::{check_kdf_policy, strict_hex, Header, Hex16, KdfBlock};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocateInfo {
    pub vault_id: [u8; 16],
    pub kdf: KdfBlock,
    pub auth_salt_mp: [u8; 16],
    pub auth_salt_rk: [u8; 16],
}

const KEYS: [&str; 4] = ["auth_salt_mp", "auth_salt_rk", "kdf", "vault_id"];
const KDF_KEYS: [&str; 7] = ["alg", "m_kib", "out_len", "p", "salt", "t", "version"];

fn keys(v: &Value) -> Option<Vec<&str>> {
    let mut k: Vec<&str> = v.as_object()?.keys().map(String::as_str).collect();
    k.sort();
    Some(k)
}

pub fn parse(json: &[u8]) -> Result<LocateInfo, ErrorCode> {
    let bad = || ErrorCode::KdfPolicyViolation;
    let v: Value = serde_json::from_slice(json).map_err(|_| bad())?;
    if keys(&v).as_deref() != Some(&KEYS[..]) || keys(&v["kdf"]).as_deref() != Some(&KDF_KEYS[..]) {
        return Err(bad());
    }
    let hex16 = |x: &Value| x.as_str().and_then(strict_hex::<16>).ok_or_else(bad);
    let kdf: KdfBlock = serde_json::from_value(v["kdf"].clone()).map_err(|_| bad())?;
    check_kdf_policy(&kdf)?;
    Ok(LocateInfo {
        vault_id: hex16(&v["vault_id"])?,
        kdf,
        auth_salt_mp: hex16(&v["auth_salt_mp"])?,
        auth_salt_rk: hex16(&v["auth_salt_rk"])?,
    })
}

/// §11.5: after the state authenticates, the committed header must match
/// the locate response exactly — else the provider tampered.
pub fn cross_check(l: &LocateInfo, committed: &Header) -> Result<(), ErrorCode> {
    let same = committed.vault_id == Hex16(l.vault_id)
        && committed.kdf == l.kdf
        && committed.auth_salt_mp == Hex16(l.auth_salt_mp)
        && committed.auth_salt_rk == Hex16(l.auth_salt_rk);
    if same {
        Ok(())
    } else {
        Err(ErrorCode::RecoveryMetadataMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn good() -> Value {
        json!({ "vault_id": "11".repeat(16),
                "kdf": {"alg":"argon2id","version":19,"m_kib":65536,"t":3,"p":1,"out_len":32,"salt":"22".repeat(16)},
                "auth_salt_mp": "33".repeat(16), "auth_salt_rk": "44".repeat(16) })
    }

    /// KD-01 / KD-02: any tuple or shape outside the allowlist stops
    /// recovery before derivation.
    #[test]
    fn kdf_policy_and_strict_shape() {
        assert!(parse(good().to_string().as_bytes()).is_ok());
        let tweaks: Vec<(&str, Value)> = vec![
            ("m_kib", json!(32768)),
            ("m_kib", json!(131072)),
            ("t", json!(2)),
            ("p", json!(4)),
            ("version", json!(16)),
            ("out_len", json!(16)),
            ("alg", json!("scrypt")),
            ("salt", json!("22".repeat(15))),
            ("salt", json!("AA".repeat(16))),
        ];
        for (k, val) in tweaks {
            let mut v = good();
            v["kdf"][k] = val.clone();
            assert_eq!(parse(v.to_string().as_bytes()), Err(ErrorCode::KdfPolicyViolation), "{k}={val}");
        }
        for (k, val) in [("vault_id", json!("11".repeat(15))), ("auth_salt_mp", json!("x")), ("extra", json!(1))] {
            let mut v = good();
            v[k] = val;
            assert_eq!(parse(v.to_string().as_bytes()), Err(ErrorCode::KdfPolicyViolation), "{k}");
        }
        let mut v = good();
        v["kdf"]["extra"] = json!(1);
        assert_eq!(parse(v.to_string().as_bytes()), Err(ErrorCode::KdfPolicyViolation));
    }
}
