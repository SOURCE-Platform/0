//! `header.json` v2 (spec v0.4 §2.10, §2.3 kdf block). The provider
//! parses the committed header blob (locate fields, KDF policy, auth-salt
//! rules, §11.3 step 7) and the helper cross-checks it at recovery
//! (§11.5), so the model lives here.

use serde::{Deserialize, Serialize};

use crate::crypto::{hex, secret};
use crate::errors::ErrorCode;
use crate::request::valid_audience;

pub const HEADER_VERSION: u32 = 2;

/// 16-byte identifier serialized as lowercase hex (32 chars).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
        strict_hex(&s).map(Hex16).ok_or_else(|| "bad hex16".to_string())
    }
}

impl From<Hex16> for String {
    fn from(h: Hex16) -> String {
        hex::encode(h.0)
    }
}

/// 32-byte digest serialized as lowercase hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Hex32(pub [u8; 32]);

impl TryFrom<String> for Hex32 {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        strict_hex(&s).map(Hex32).ok_or_else(|| "bad hex32".to_string())
    }
}

impl From<Hex32> for String {
    fn from(h: Hex32) -> String {
        hex::encode(h.0)
    }
}

/// Lowercase hex only (§11.5 rule 1: "exact keys, lowercase hex").
pub fn strict_hex<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.bytes().any(|c| c.is_ascii_uppercase()) {
        return None;
    }
    hex::decode_array(s)
}

/// §2.3 kdf block, exactly as the header and the locate response carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KdfBlock {
    pub alg: String,
    /// Argon2 version 0x13.
    pub version: u32,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    pub out_len: u32,
    pub salt: Hex16,
}

/// The compiled-in allowlist (§2.3): in v1 exactly the frozen tuple.
pub const ALLOWED_TUPLES: [(u32, u32, u32, u32); 1] = [(65536, 3, 1, 32)];

impl KdfBlock {
    pub fn frozen(salt: [u8; 16]) -> KdfBlock {
        let (m_kib, t, p, out_len) = ALLOWED_TUPLES[0];
        KdfBlock { alg: "argon2id".into(), version: 0x13, m_kib, t, p, out_len, salt: Hex16(salt) }
    }

    pub fn fresh() -> KdfBlock {
        KdfBlock::frozen(secret::random_salt())
    }

    /// Exact equality with an allowlisted tuple — never "at least as strong".
    pub fn is_allowlisted(&self) -> bool {
        self.alg == "argon2id"
            && self.version == 0x13
            && ALLOWED_TUPLES.contains(&(self.m_kib, self.t, self.p, self.out_len))
    }
}

/// All header v2 fields (§2.10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub vault_id: Hex16,
    pub version: u32,
    pub kdf: KdfBlock,
    pub meta_salt: Hex16,
    pub import_fp_salt: Hex16,
    pub auth_salt_mp: Hex16,
    pub auth_salt_rk: Hex16,
    /// The provider origin this vault was set up against (§11.4).
    pub provider: String,
    pub vk_generation: u32,
    pub registry_head: Hex32,
    pub manifest_generation: u64,
}

impl Header {
    /// Fresh header for `setup_vault`: random salts, the frozen KDF tuple.
    pub fn fresh(vault_id: Hex16, provider: &str) -> Header {
        Header {
            vault_id,
            version: HEADER_VERSION,
            kdf: KdfBlock::fresh(),
            meta_salt: Hex16::random(),
            import_fp_salt: Hex16::random(),
            auth_salt_mp: Hex16::random(),
            auth_salt_rk: Hex16::random(),
            provider: provider.to_string(),
            vk_generation: 1,
            registry_head: Hex32([0u8; 32]),
            manifest_generation: 1,
        }
    }
}

/// Structural parse (§3.5): version 1 is the retired format
/// (`FORMAT_INVALID`), above 2 is `FORMAT_TOO_NEW`; unknown fields,
/// uppercase hex or a malformed provider origin are `FORMAT_INVALID`.
/// The KDF allowlist is a separate, mandatory check (`check_kdf_policy`),
/// because the provider and the helper report it differently.
pub fn parse_header(bytes: &[u8]) -> Result<Header, ErrorCode> {
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| ErrorCode::FormatInvalid)?;
    match v.get("version").and_then(serde_json::Value::as_u64) {
        Some(2) => {}
        Some(n) if n > 2 => return Err(ErrorCode::FormatTooNew),
        _ => return Err(ErrorCode::FormatInvalid),
    }
    let h: Header = serde_json::from_value(v).map_err(|_| ErrorCode::FormatInvalid)?;
    if !valid_audience(&h.provider) {
        return Err(ErrorCode::FormatInvalid);
    }
    Ok(h)
}

/// §2.3 client allowlist: refuse before any MP derivation.
pub fn check_kdf_policy(kdf: &KdfBlock) -> Result<(), ErrorCode> {
    if kdf.is_allowlisted() {
        Ok(())
    } else {
        Err(ErrorCode::KdfPolicyViolation)
    }
}

pub fn write_header(header: &Header) -> Result<Vec<u8>, ErrorCode> {
    serde_json::to_vec_pretty(header).map_err(|_| ErrorCode::Internal)
}

/// `password.wrap` / `recovery.wrap` public JSON (§2.5). The ciphertext is
/// opaque here; the provider reads only the MP wrap's parameter block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrapKdf {
    pub m: u32,
    pub t: u32,
    pub p: u32,
    pub salt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordWrapFile {
    pub v: u32,
    pub kind: String,
    pub kdf_version: u32,
    pub argon2id: WrapKdf,
    pub nonce: String,
    pub ct: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryWrapFile {
    pub v: u32,
    pub kind: String,
    pub salt: String,
    pub nonce: String,
    pub ct: String,
}

/// §2.3/§11.3 step 7: the MP wrap's public block (m, t, p, salt) equals
/// the header's `kdf` block.
pub fn mp_wrap_matches(wrap: &[u8], kdf: &KdfBlock) -> bool {
    let Ok(w) = serde_json::from_slice::<PasswordWrapFile>(wrap) else {
        return false;
    };
    w.kind == "mp"
        && w.argon2id.m == kdf.m_kib
        && w.argon2id.t == kdf.t
        && w.argon2id.p == kdf.p
        && strict_hex::<16>(&w.argon2id.salt) == Some(kdf.salt.0)
}
