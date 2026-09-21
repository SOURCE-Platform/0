//! P-256 ECDSA helpers implementing the §2.7 wire policy:
//! - public keys are the 65-byte uncompressed X9.63 form (`0x04‖X‖Y`),
//!   on-curve checked (§4.4 rule 8);
//! - signatures are 64-byte `r‖s`, **canonical low-S**; verifiers reject
//!   high-S so bytes inside hash-chained objects are unique;
//! - the signed payload is always a domain-separated SHA-256 digest
//!   (prehash), never a raw message.
//!
//! Production signing happens in the Secure Enclave (Phase E). The
//! software `SigningKey` path here exists for test/vector generation only
//! (spec §2.1: Rust p256 = verify/normalize); nothing in the production
//! helper calls it.

use p256::ecdsa::signature::hazmat::{PrehashSigner, PrehashVerifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};

use super::CryptoError;

pub const PUBKEY_LEN: usize = 65;
pub const SIGNATURE_LEN: usize = 64;
pub const DIGEST_LEN: usize = 32;

/// Parse a 65-byte uncompressed P-256 public key; rejects wrong length,
/// wrong prefix, and off-curve points (§4.4 rule 8).
pub fn parse_verifying_key(bytes: &[u8]) -> Result<VerifyingKey, CryptoError> {
    if bytes.len() != PUBKEY_LEN || bytes[0] != 0x04 {
        return Err(CryptoError::InvalidKeyEncoding);
    }
    // sec1 decoding validates prefix, coordinates, and on-curve membership.
    VerifyingKey::from_sec1_bytes(bytes).map_err(|_| CryptoError::InvalidKeyEncoding)
}

/// True iff the `s` half of a 64-byte `r‖s` signature is in low-S form.
pub fn is_low_s(signature: &[u8]) -> bool {
    let Some(sig) = signature_from_bytes(signature) else {
        return false;
    };
    // ecdsa 0.17: normalize_s() always returns the low-S form; equality
    // means the input was already canonical.
    sig.normalize_s() == sig
}

fn signature_from_bytes(bytes: &[u8]) -> Option<Signature> {
    let array: &[u8; SIGNATURE_LEN] = bytes.try_into().ok()?;
    Signature::from_slice(array).ok()
}

/// Normalize a raw 64-byte `r‖s` ECDSA signature to canonical low-S
/// (§2.7 producer rule). Secure Enclave / CryptoKit signing is randomized
/// and may return either form, so every SE signature passes through here
/// before it is embedded in a hash-chained object.
pub fn normalize_low_s(raw: &[u8; SIGNATURE_LEN]) -> Result<[u8; SIGNATURE_LEN], CryptoError> {
    let sig = signature_from_bytes(raw).ok_or(CryptoError::InvalidKeyEncoding)?;
    let sig = sig.normalize_s();
    let mut out = [0u8; SIGNATURE_LEN];
    out[..32].copy_from_slice(&sig.r().to_bytes());
    out[32..].copy_from_slice(&sig.s().to_bytes());
    Ok(out)
}

/// Verify an ECDSA signature over a domain-separated digest. High-S
/// signatures are rejected before verification (§2.7 canonical wire form).
pub fn verify_prehash(
    pubkey65: &[u8],
    digest32: &[u8; DIGEST_LEN],
    signature: &[u8],
) -> Result<(), CryptoError> {
    let key = parse_verifying_key(pubkey65)?;
    let sig = signature_from_bytes(signature).ok_or(CryptoError::InvalidKeyEncoding)?;
    if !is_low_s(signature) {
        return Err(CryptoError::NonCanonicalSignature);
    }
    key.verify_prehash(digest32, &sig)
        .map_err(|_| CryptoError::SignatureInvalid)
}

/// Dev/test-only software signing (vectors, tests). Deterministic RFC 6979
/// from `p256`; normalized to low-S on output (§2.7 producer rule).
pub fn dev_sign_prehash(
    signing_key: &SigningKey,
    digest32: &[u8; DIGEST_LEN],
) -> [u8; SIGNATURE_LEN] {
    let sig: Signature = signing_key
        .sign_prehash(digest32)
        .expect("prehash is 32 bytes");
    let sig = sig.normalize_s();
    let mut out = [0u8; SIGNATURE_LEN];
    out[..32].copy_from_slice(&sig.r().to_bytes());
    out[32..].copy_from_slice(&sig.s().to_bytes());
    out
}

/// Dev/test-only: software signing keypair from a fixed scalar (vectors
/// need reproducible keys; production keys are SE-generated, Phase E).
pub fn dev_keypair_from_scalar(scalar: [u8; 32]) -> (SigningKey, [u8; PUBKEY_LEN]) {
    let field_bytes = p256::FieldBytes::from(scalar);
    let signing = SigningKey::from_bytes(&field_bytes).expect("test scalar is in range");
    let sec1 = signing.verifying_key().to_sec1_bytes();
    let mut pubkey = [0u8; PUBKEY_LEN];
    pubkey.copy_from_slice(&sec1);
    (signing, pubkey)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn digest(msg: &[u8]) -> [u8; 32] {
        Sha256::digest(msg).into()
    }

    #[test]
    fn sign_verify_roundtrip_low_s() {
        let (signing, pubkey) = dev_keypair_from_scalar([7u8; 32]);
        let sig = dev_sign_prehash(&signing, &digest(b"test"));
        assert!(is_low_s(&sig));
        assert!(verify_prehash(&pubkey, &digest(b"test"), &sig).is_ok());
    }

    #[test]
    fn high_s_is_rejected() {
        let (signing, pubkey) = dev_keypair_from_scalar([7u8; 32]);
        let sig = dev_sign_prehash(&signing, &digest(b"test"));
        // Forge the high-S twin s' = n − s via scalar-field negation:
        // valid ECDSA, non-canonical wire form (§2.7).
        let parsed = Signature::from_slice(&sig).unwrap();
        let flipped_s: p256::Scalar = -*parsed.s();
        let forged: [u8; SIGNATURE_LEN] =
            Signature::from_scalars(parsed.r().to_bytes(), flipped_s.to_bytes())
                .unwrap()
                .to_bytes()
                .into();
        assert!(!is_low_s(&forged));
        assert_eq!(
            verify_prehash(&pubkey, &digest(b"test"), &forged),
            Err(CryptoError::NonCanonicalSignature)
        );
    }

    #[test]
    fn off_curve_and_short_keys_rejected() {
        assert!(parse_verifying_key(&[0x04; 65]).is_err()); // (0,0) not on curve
        assert!(parse_verifying_key(&[0x04; 64]).is_err()); // too short
        assert!(parse_verifying_key(&[0x05; 65]).is_err()); // bad prefix
        let (_, pubkey) = dev_keypair_from_scalar([9u8; 32]);
        assert!(parse_verifying_key(&pubkey).is_ok());
    }

    #[test]
    fn wrong_digest_fails_verification() {
        let (signing, pubkey) = dev_keypair_from_scalar([3u8; 32]);
        let sig = dev_sign_prehash(&signing, &digest(b"a"));
        assert_eq!(
            verify_prehash(&pubkey, &digest(b"b"), &sig),
            Err(CryptoError::SignatureInvalid)
        );
    }

    #[test]
    fn normalize_low_s_canonicalizes_high_s() {
        let (signing, pubkey) = dev_keypair_from_scalar([11u8; 32]);
        let sig = dev_sign_prehash(&signing, &digest(b"se"));
        let parsed = Signature::from_slice(&sig).unwrap();
        let flipped_s: p256::Scalar = -*parsed.s();
        let high: [u8; SIGNATURE_LEN] =
            Signature::from_scalars(parsed.r().to_bytes(), flipped_s.to_bytes())
                .unwrap()
                .to_bytes()
                .into();
        assert!(!is_low_s(&high));
        let fixed = normalize_low_s(&high).unwrap();
        assert!(is_low_s(&fixed));
        assert_eq!(fixed, sig, "both forms normalize to the same canonical bytes");
        assert!(verify_prehash(&pubkey, &digest(b"se"), &fixed).is_ok());
        // Already-canonical input is unchanged.
        assert_eq!(normalize_low_s(&sig).unwrap(), sig);
    }
}
