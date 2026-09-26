//! HKDF-SHA-256 subkey/KEK derivations with the §2.9 context registry.
//! The info strings below are the registry — new derivations add a row
//! here and in the spec, never an ad-hoc string at a call site.

use hkdf::Hkdf;
use sha2::Sha256;

use super::secret::SecretBytes;
use super::CryptoError;

pub const INFO_WRAP_MP: &[u8] = b"ov0/wrap/mp/v1";
pub const INFO_WRAP_RK: &[u8] = b"ov0/wrap/rk/v1";
pub const INFO_RECORD: &[u8] = b"ov0/record/v1";
pub const INFO_META: &[u8] = b"ov0/meta/v1";
pub const INFO_RECOVERY_AUTH: &[u8] = b"ov0/recovery-auth/v1";
pub const INFO_IMPORT_FINGERPRINT: &[u8] = b"ov0/import-fingerprint/v1";
pub const INFO_ENROLL_SAS: &[u8] = b"ov0/enroll/sas/v1";
/// Provider recovery-auth `ikm_c` (§11.4); the info is this prefix ‖ vault_id.
pub const INFO_PROVIDER_AUTH_MP: &[u8] = b"ov0/provider-recovery-auth/mp/v2";
pub const INFO_PROVIDER_AUTH_RK: &[u8] = b"ov0/provider-recovery-auth/rk/v2";

/// All §2.9 strings (v0.4); XV-HKDF vectors cover every one of them. The
/// v0.3 `backup-auth`, `locate`, `locator` and `device-creds` contexts are
/// retired and appear nowhere.
pub const ALL_INFO_STRINGS: [&[u8]; 9] = [
    INFO_WRAP_MP,
    INFO_WRAP_RK,
    INFO_RECORD,
    INFO_META,
    INFO_RECOVERY_AUTH,
    INFO_IMPORT_FINGERPRINT,
    INFO_ENROLL_SAS,
    INFO_PROVIDER_AUTH_MP,
    INFO_PROVIDER_AUTH_RK,
];

/// HKDF-SHA-256 extract+expand to a 32-byte secret.
pub fn hkdf32(ikm: &[u8], salt: &[u8], info: &[u8]) -> Result<SecretBytes<32>, CryptoError> {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut out = [0u8; 32];
    hk.expand(info, &mut out)
        .map_err(|_| CryptoError::IntegrityFailure)?;
    Ok(SecretBytes::new(out))
}

// --- Named derivations (§2.5, §2.6, §2.9) --------------------------------

/// password.wrap wrap key: HKDF(ikm=PK, salt=argon2id.salt, info=§2.9).
pub fn wrap_key_mp(
    pk: &SecretBytes<32>,
    kdf_salt: &[u8; 16],
) -> Result<SecretBytes<32>, CryptoError> {
    hkdf32(pk.expose(), kdf_salt, INFO_WRAP_MP)
}

/// recovery.wrap wrap key: HKDF(ikm=RK, salt=stored random 16B).
pub fn wrap_key_rk(rk: &SecretBytes<32>, salt: &[u8; 16]) -> Result<SecretBytes<32>, CryptoError> {
    hkdf32(rk.expose(), salt, INFO_WRAP_RK)
}

/// Per-record subkey (§2.6): HKDF(ikm=VK, salt=record_id_bytes).
pub fn record_key(
    vk: &SecretBytes<32>,
    record_id: &[u8; 16],
) -> Result<SecretBytes<32>, CryptoError> {
    hkdf32(vk.expose(), record_id, INFO_RECORD)
}

/// Metadata key (§2.6): HKDF(ikm=VK, salt=header.meta_salt).
pub fn meta_key(
    vk: &SecretBytes<32>,
    meta_salt: &[u8; 16],
) -> Result<SecretBytes<32>, CryptoError> {
    hkdf32(vk.expose(), meta_salt, INFO_META)
}

/// Registry recovery-epoch proof key (§4.5): HKDF(ikm=VK, salt=manifest_hash).
pub fn recovery_auth_key(
    vk: &SecretBytes<32>,
    manifest_hash: &[u8; 32],
) -> Result<SecretBytes<32>, CryptoError> {
    hkdf32(vk.expose(), manifest_hash, INFO_RECOVERY_AUTH)
}

/// Import idempotency HMAC key from VK (§10.3).
pub fn import_fp_key(
    vk: &SecretBytes<32>,
    salt: &[u8; 16],
) -> Result<SecretBytes<32>, CryptoError> {
    hkdf32(vk.expose(), salt, INFO_IMPORT_FINGERPRINT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivations_are_domain_separated() {
        let k = SecretBytes::new([3u8; 32]);
        let salt = [4u8; 16];
        let outs: Vec<[u8; 32]> = ALL_INFO_STRINGS.iter().map(|i| *hkdf32(k.expose(), &salt, i).unwrap().expose()).collect();
        for (i, a) in outs.iter().enumerate() {
            assert!(outs[i + 1..].iter().all(|b| b != a), "context {i} collides");
        }
    }

    #[test]
    fn salt_changes_output() {
        let pk = SecretBytes::new([5u8; 32]);
        let a = wrap_key_rk(&pk, &[1u8; 16]).unwrap();
        let b = wrap_key_rk(&pk, &[2u8; 16]).unwrap();
        assert_ne!(a.expose(), b.expose());
    }
}
