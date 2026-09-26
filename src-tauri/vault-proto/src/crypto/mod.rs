//! Cryptographic primitives shared by the helper and the provider (spec
//! §2, §4.2): TLV, hex, ECDSA P-256, HKDF contexts, zeroizing secret types
//! and the registry entry codec. Every secret lives in a zeroizing type
//! (§2.11); no secret type implements `Debug`.

pub mod ecdsa;
pub mod hex;
pub mod hkdf;
pub mod registry;
pub mod secret;
pub mod recovery_auth;
pub mod tlv;

/// Crypto-layer errors. These map to §15 user-facing codes at the op
/// layer; the crypto core itself never decides UX.
#[derive(Debug, PartialEq, Eq)]
pub enum CryptoError {
    /// AEAD tag verification failed (wrong key, tampering, wrong AAD).
    /// Maps to WRONG_CREDENTIAL (unwrap) or RECORD_CORRUPT (records).
    IntegrityFailure,
    /// kdf_version downgrade attempt (§2.3, CR-09).
    KdfDowngrade,
    /// Argon2 parameter block malformed or out of range.
    KdfParams,
    /// TLV strict-decode violation (§4.2).
    Tlv(tlv::TlvError),
    /// BIP-39 checksum/wordlist failure (§2.4 → §15 RECOVERY_KEY_INVALID).
    RecoveryKeyInvalid,
    /// A required field is absent for this entry/wrap kind, or a forbidden
    /// field is present (§2.2 payload split, §4.3 presence rules).
    FieldPresence,
    /// P-256 point or signature encoding invalid (§2.7, §4.4 rule 8).
    InvalidKeyEncoding,
    /// High-S signature presented where canonical form is required (§2.7).
    NonCanonicalSignature,
    /// Signature or proof verification failed (maps to SIGNATURE_INVALID).
    SignatureInvalid,
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CryptoError::IntegrityFailure => write!(f, "integrity check failed"),
            CryptoError::KdfDowngrade => write!(f, "kdf_version downgrade refused"),
            CryptoError::KdfParams => write!(f, "argon2id parameter block invalid"),
            CryptoError::Tlv(e) => write!(f, "tlv: {e}"),
            CryptoError::RecoveryKeyInvalid => write!(f, "not a valid recovery key"),
            CryptoError::FieldPresence => write!(f, "field presence rule violated"),
            CryptoError::InvalidKeyEncoding => write!(f, "invalid key encoding"),
            CryptoError::NonCanonicalSignature => write!(f, "non-canonical (high-S) signature"),
            CryptoError::SignatureInvalid => write!(f, "signature invalid"),
        }
    }
}

impl std::error::Error for CryptoError {}

impl From<tlv::TlvError> for CryptoError {
    fn from(e: tlv::TlvError) -> Self {
        CryptoError::Tlv(e)
    }
}
