//! `header.json` model (spec §2.10 fields, §2.3 kdf block) and the §3.5
//! fail-closed versioning rule.

use serde::{Deserialize, Serialize};

use crate::crypto::kdf::{self, Argon2Params};
use crate::crypto::{hex, secret};
use crate::errors::ErrorCode;


/// 16-byte identifier serialized as lowercase hex (32 chars).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Hex16(pub [u8; 16]);

impl Hex16 {
    pub fn random() -> Self {
        Hex16(secret::random_salt())
    }
}

impl TryFrom<String> for Hex16 {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        hex::decode_array(&s).map(Hex16).ok_or_else(|| "bad hex16".to_string())
    }
}

impl From<Hex16> for String {
    fn from(h: Hex16) -> String {
        hex::encode(h.0)
    }
}

/// 32-byte digest serialized as lowercase hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Hex32(pub [u8; 32]);

impl TryFrom<String> for Hex32 {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        hex::decode_array(&s).map(Hex32).ok_or_else(|| "bad hex32".to_string())
    }
}

impl From<Hex32> for String {
    fn from(h: Hex32) -> String {
        hex::encode(h.0)
    }
}

/// KDF block exactly as §2.3 stores it in `header.json`:
/// `{kdf: "argon2id", kdf_version: 1, m, t, p, salt}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfBlock {
    pub kdf: String,
    pub kdf_version: u32,
    pub m: u32,
    pub t: u32,
    pub p: u32,
    /// hex, 16 bytes
    pub salt: String,
}

impl KdfBlock {
    /// Fresh v1 block with a random salt (§2.3; tuple provisional per
    /// `docs/security/argon2-calibration.md`).
    pub fn v1() -> Self {
        KdfBlock {
            kdf: "argon2id".to_string(),
            kdf_version: kdf::KDF_VERSION_V1,
            m: Argon2Params::V1.m,
            t: Argon2Params::V1.t,
            p: Argon2Params::V1.p,
            salt: hex::encode(secret::random_salt()),
        }
    }

    pub fn params(&self) -> Argon2Params {
        Argon2Params {
            m: self.m,
            t: self.t,
            p: self.p,
        }
    }

    pub fn salt_bytes(&self) -> Result<[u8; 16], ErrorCode> {
        hex::decode_array(&self.salt).ok_or(ErrorCode::DbCorrupt)
    }
}

/// All header fields named per §2.10.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    /// uuid bytes, hex.
    pub vault_id: Hex16,
    pub version: u32,
    pub kdf: KdfBlock,
    pub meta_salt: Hex16,
    pub import_fp_salt: Hex16,
    pub locator_salt_mp: Hex16,
    pub locator_salt_rk: Hex16,
    pub vk_generation: u32,
    pub registry_head: Hex32,
    pub manifest_generation: u64,
}

impl Header {
    /// Fresh header for `setup_vault` (§5.4 minus RK parts, which are
    /// Phase D): version 1, random salts, empty registry head (no
    /// enrolled device exists yet — the genesis entry lands with Phase E
    /// enrollment; documented deviation), manifest generation 1.
    pub fn fresh(vault_id: Hex16) -> Self {
        Header {
            vault_id,
            version: 1,
            kdf: KdfBlock::v1(),
            meta_salt: Hex16::random(),
            import_fp_salt: Hex16::random(),
            locator_salt_mp: Hex16::random(),
            locator_salt_rk: Hex16::random(),
            vk_generation: 1,
            registry_head: Hex32([0u8; 32]),
            manifest_generation: 1,
        }
    }
}

/// §3.5: strict parse; unknown/newer format versions fail closed with
/// `FORMAT_TOO_NEW`. Structurally unreadable headers map to `DB_CORRUPT`
/// (§15 has no header-specific code; the corruption class is the
/// documented mapping — §3.6 ERROR-state path).
pub fn parse_header(bytes: &[u8]) -> Result<Header, ErrorCode> {
    let header: Header = serde_json::from_slice(bytes).map_err(|_| ErrorCode::DbCorrupt)?;
    if header.version != 1 {
        return Err(ErrorCode::FormatTooNew);
    }
    if header.kdf.kdf != "argon2id" || header.kdf.kdf_version != kdf::KDF_VERSION_V1 {
        return Err(ErrorCode::FormatTooNew);
    }
    // §3.5/§16.16 SC-03: import_fp_salt is a hard-required 16-byte field.
    header.sanity_lengths()?;
    Ok(header)
}

impl Header {
    fn sanity_lengths(&self) -> Result<(), ErrorCode> {
        // Hex16/Hex32 TryFrom already enforces byte lengths at parse time;
        // kdf salt must be exactly 16 bytes.
        self.kdf.salt_bytes()?;
        if self.kdf.m == 0 || self.kdf.t == 0 || self.kdf.p == 0 {
            return Err(ErrorCode::DbCorrupt);
        }
        Ok(())
    }
}

pub fn write_header(header: &Header) -> Result<Vec<u8>, ErrorCode> {
    serde_json::to_vec_pretty(header).map_err(|_| ErrorCode::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_header() {
        let h = Header::fresh(Hex16::random());
        let bytes = write_header(&h).unwrap();
        let parsed = parse_header(&bytes).unwrap();
        assert_eq!(parsed.vault_id, h.vault_id);
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.kdf.m, Argon2Params::V1.m);
        assert_eq!(parsed.manifest_generation, 1);
    }

    #[test]
    fn future_version_refused_format_too_new() {
        for v in [0u32, 2, 100] {
            let mut h = Header::fresh(Hex16::random());
            h.version = v;
            let bytes = write_header(&h).unwrap();
            assert_eq!(parse_header(&bytes), Err(ErrorCode::FormatTooNew), "v={v}");
        }
    }

    #[test]
    fn corrupt_header_refused() {
        assert_eq!(parse_header(b"{not json"), Err(ErrorCode::DbCorrupt));
        assert_eq!(parse_header(b"[]"), Err(ErrorCode::DbCorrupt));
    }

    #[test]
    fn unknown_kdf_refused_as_too_new() {
        let mut h = Header::fresh(Hex16::random());
        h.kdf.kdf_version = 2;
        let bytes = write_header(&h).unwrap();
        assert_eq!(parse_header(&bytes), Err(ErrorCode::FormatTooNew));
        let mut h = Header::fresh(Hex16::random());
        h.kdf.kdf = "scrypt".to_string();
        let bytes = write_header(&h).unwrap();
        assert_eq!(parse_header(&bytes), Err(ErrorCode::FormatTooNew));
    }

    #[test]
    fn import_fp_salt_length_enforced() {
        // SC-03: 15-byte salt must fail strict parse.
        let h = Header::fresh(Hex16::random());
        let mut v: serde_json::Value = serde_json::from_slice(&write_header(&h).unwrap()).unwrap();
        v["import_fp_salt"] = serde_json::Value::String("aabbcc".to_string());
        assert_eq!(
            parse_header(serde_json::to_string(&v).unwrap().as_bytes()),
            Err(ErrorCode::DbCorrupt)
        );
    }
}
