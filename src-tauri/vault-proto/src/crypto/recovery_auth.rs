//! Recovery-class provider keys (spec v0.4 §11.4, owner decision S-1).
//!
//! ```text
//! ikm_c        = HKDF-SHA256(ikm = secret_c, salt = auth_salt_c, info = domain_c ‖ vault_id, L = 32)
//! (sk_c, pk_c) = DeriveKeyPair(ikm_c)    # RFC 9180 §7.1.3, DHKEM(P-256, HKDF-SHA256)
//! key_id_c     = SHA-256(pk_c)
//! ```
//!
//! `secret_c` is PK (MP class) or RK_bytes (RK class). The MP-class key is
//! in the same password-guessing class as `password.wrap`; nothing here
//! adds entropy. `ikm_c` and `sk_c` live only in zeroizing types and are
//! never persisted or sent over IPC (§2.11).

use hkdf::Hkdf;
use p256::ecdsa::SigningKey;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::ecdsa::{self, PUBKEY_LEN, SIGNATURE_LEN};
use super::hkdf::hkdf32;
use super::secret::SecretBytes;
use super::CryptoError;

/// `signer_class` values (§11.4 `ProviderRequest` 0x07); the same byte is
/// the `class` in `recovery_auth_digest` and `recovery_auth_updates`.
pub const CLASS_DEVICE: u8 = 1;
pub const CLASS_MP: u8 = 2;
pub const CLASS_RK: u8 = 3;

pub const DOMAIN_MP: &[u8] = b"ov0/provider-recovery-auth/mp/v2";
pub const DOMAIN_RK: &[u8] = b"ov0/provider-recovery-auth/rk/v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecoveryClass {
    Mp,
    Rk,
}

impl RecoveryClass {
    pub fn code(self) -> u8 {
        match self {
            RecoveryClass::Mp => CLASS_MP,
            RecoveryClass::Rk => CLASS_RK,
        }
    }

    pub fn from_code(c: u8) -> Option<RecoveryClass> {
        match c {
            CLASS_MP => Some(RecoveryClass::Mp),
            CLASS_RK => Some(RecoveryClass::Rk),
            _ => None,
        }
    }

    fn domain(self) -> &'static [u8] {
        match self {
            RecoveryClass::Mp => DOMAIN_MP,
            RecoveryClass::Rk => DOMAIN_RK,
        }
    }
}

/// A derived recovery-auth key pair. `SigningKey` zeroizes on drop.
pub struct RecoveryAuthKey {
    signing: SigningKey,
    pub public: [u8; PUBKEY_LEN],
}

impl RecoveryAuthKey {
    pub fn key_id(&self) -> [u8; 32] {
        key_id(&self.public)
    }

    /// ECDSA-P256-SHA256 over a prehash, RFC 6979 deterministic nonce,
    /// low-S (§11.4).
    pub fn sign_prehash(&self, digest: &[u8; 32]) -> [u8; SIGNATURE_LEN] {
        ecdsa::dev_sign_prehash(&self.signing, digest)
    }
}

pub fn key_id(public: &[u8; PUBKEY_LEN]) -> [u8; 32] {
    Sha256::digest(public).into()
}

/// §11.4: `ikm_c` from the class secret, then `DeriveKeyPair`.
pub fn derive(
    class: RecoveryClass,
    secret: &SecretBytes<32>,
    auth_salt: &[u8; 16],
    vault_id: &[u8; 16],
) -> Result<RecoveryAuthKey, CryptoError> {
    let mut info = class.domain().to_vec();
    info.extend_from_slice(vault_id);
    let ikm = hkdf32(secret.expose(), auth_salt, &info)?;
    derive_key_pair(ikm.expose())
}

const SUITE_ID: &[u8] = b"KEM\x00\x10"; // "KEM" ‖ I2OSP(0x0010, 2)

/// RFC 9180 §7.1.3 `DeriveKeyPair` for DHKEM(P-256, HKDF-SHA256).
pub fn derive_key_pair(ikm: &[u8]) -> Result<RecoveryAuthKey, CryptoError> {
    let mut labeled_ikm = Zeroizing::new(b"HPKE-v1".to_vec());
    labeled_ikm.extend_from_slice(SUITE_ID);
    labeled_ikm.extend_from_slice(b"dkp_prk");
    labeled_ikm.extend_from_slice(ikm);
    let (_, hk) = Hkdf::<Sha256>::extract(Some(&[]), &labeled_ikm);
    derive_from_candidates(|counter| {
        let mut info = Vec::with_capacity(2 + 7 + SUITE_ID.len() + 9 + 1);
        info.extend_from_slice(&32u16.to_be_bytes()); // I2OSP(L, 2)
        info.extend_from_slice(b"HPKE-v1");
        info.extend_from_slice(SUITE_ID);
        info.extend_from_slice(b"candidate");
        info.push(counter);
        let mut bytes = Zeroizing::new([0u8; 32]);
        hk.expand(&info, bytes.as_mut()).expect("32 bytes is a valid HKDF-SHA256 length");
        bytes
    })
}

/// The rejection loop over an injectable candidate source (CR-13 (c)):
/// counter 0..=255; `bytes[0] &= 0xFF` (the P-256 bitmask, a no-op);
/// accept the first candidate with 0 < sk < n.
#[doc(hidden)]
pub fn derive_from_candidates(
    mut candidate: impl FnMut(u8) -> Zeroizing<[u8; 32]>,
) -> Result<RecoveryAuthKey, CryptoError> {
    for counter in 0..=255u8 {
        let bytes = candidate(counter);
        // `SigningKey::from_bytes` accepts exactly the scalars 0 < sk < n.
        if let Ok(signing) = SigningKey::from_bytes(&p256::FieldBytes::from(*bytes)) {
            let sec1 = signing.verifying_key().to_sec1_bytes();
            let mut public = [0u8; PUBKEY_LEN];
            public.copy_from_slice(&sec1);
            return Ok(RecoveryAuthKey { signing, public });
        }
    }
    Err(CryptoError::InvalidKeyEncoding) // DeriveKeyPairError
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::hex;

    /// RFC 9180 Appendix A.3.1 (DHKEM(P-256, HKDF-SHA256)): ikmE/ikmR →
    /// pkEm/pkRm; the private scalars are checked through the public keys
    /// they generate (sk never leaves `RecoveryAuthKey`).
    #[test]
    fn rfc9180_a3_derive_key_pair() {
        let cases = [
            (
                "4270e54ffd08d79d5928020af4686d8f6b7d35dbe470265f1f5aa22816ce860e",
                "04a92719c6195d5085104f469a8b9814d5838ff72b60501e2c4466e5e67b325ac98536d7b61a1af4b78e5b7f951c0900be863c403ce65c9bfcb9382657222d18c4",
            ),
            (
                "668b37171f1072f3cf12ea8a236a45df23fc13b82af3609ad1e354f6ef817550",
                "04fe8c19ce0905191ebc298a9245792531f26f0cece2460639e8bc39cb7f706a826a779b4cf969b8a0e539c7f62fb3d30ad6aa8f80e30f1d128aafd68a2ce72ea0",
            ),
        ];
        for (ikm, pk) in cases {
            let k = derive_key_pair(&hex::decode(ikm).unwrap()).unwrap();
            assert_eq!(hex::encode(k.public), pk);
        }
    }

    /// CR-13 (c): out-of-range candidates (0 and ≥ n) are skipped; the
    /// counter advances; 256 bad candidates is DeriveKeyPairError.
    #[test]
    fn rejection_loop() {
        let n_bytes = hex::decode_array::<32>("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551").unwrap();
        let mut seen = Vec::new();
        let k = derive_from_candidates(|c| {
            seen.push(c);
            Zeroizing::new(match c {
                0 => [0u8; 32],
                1 => n_bytes,
                2 => [0xff; 32],
                _ => [0x11; 32],
            })
        })
        .unwrap();
        assert_eq!(seen, vec![0, 1, 2, 3]);
        let (_, expect) = ecdsa::dev_keypair_from_scalar([0x11; 32]);
        assert_eq!(k.public, expect);
        assert!(derive_from_candidates(|_| Zeroizing::new([0u8; 32])).is_err());
    }
}
