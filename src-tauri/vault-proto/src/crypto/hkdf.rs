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
pub const INFO_LOCATE_MP: &[u8] = b"ov0/locate/mp/v1";
pub const INFO_LOCATE_RK: &[u8] = b"ov0/locate/rk/v1";
pub const INFO_BACKUP_AUTH_MP: &[u8] = b"ov0/backup-auth/mp/v1";
pub const INFO_BACKUP_AUTH_RK: &[u8] = b"ov0/backup-auth/rk/v1";
pub const INFO_IMPORT_FINGERPRINT: &[u8] = b"ov0/import-fingerprint/v1";
pub const INFO_ENROLL_SAS: &[u8] = b"ov0/enroll/sas/v1";
/// Key for the authorizing device's record of the per-device backup
/// credentials it has issued (§11.4; added in Phase E).
pub const INFO_DEVICE_CREDS: &[u8] = b"ov0/device-creds/v1";

/// All §2.9 strings; XV-HKDF vectors cover every one of them.
pub const ALL_INFO_STRINGS: [&[u8]; 12] = [
    INFO_WRAP_MP,
    INFO_WRAP_RK,
    INFO_RECORD,
    INFO_META,
    INFO_RECOVERY_AUTH,
    INFO_LOCATE_MP,
    INFO_LOCATE_RK,
    INFO_BACKUP_AUTH_MP,
    INFO_BACKUP_AUTH_RK,
    INFO_IMPORT_FINGERPRINT,
    INFO_ENROLL_SAS,
    INFO_DEVICE_CREDS,
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

/// Recovery-class backup credential from PK (§11.4, CR-13).
pub fn backup_cred_mp(
    pk: &SecretBytes<32>,
    salt: &[u8; 16],
) -> Result<SecretBytes<32>, CryptoError> {
    hkdf32(pk.expose(), salt, INFO_BACKUP_AUTH_MP)
}

/// Recovery-class backup credential from the RK-derived key (§11.4, CR-13).
pub fn backup_cred_rk(
    rk: &SecretBytes<32>,
    salt: &[u8; 16],
) -> Result<SecretBytes<32>, CryptoError> {
    hkdf32(rk.expose(), salt, INFO_BACKUP_AUTH_RK)
}

/// Recovery locator keys (§12 scenario 3).
pub fn locator_mp(pk: &SecretBytes<32>, salt: &[u8; 16]) -> Result<SecretBytes<32>, CryptoError> {
    hkdf32(pk.expose(), salt, INFO_LOCATE_MP)
}

pub fn locator_rk(rk: &SecretBytes<32>, salt: &[u8; 16]) -> Result<SecretBytes<32>, CryptoError> {
    hkdf32(rk.expose(), salt, INFO_LOCATE_RK)
}

/// Key sealing the issued-credential record (§11.4): HKDF(ikm=VK,
/// salt=vault_id). Rotates with the VK, like every other VK-derived key.
pub fn device_creds_key(
    vk: &SecretBytes<32>,
    vault_id: &[u8; 16],
) -> Result<SecretBytes<32>, CryptoError> {
    hkdf32(vk.expose(), vault_id, INFO_DEVICE_CREDS)
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
        let vk = SecretBytes::new([3u8; 32]);
        let salt = [4u8; 16];
        let a = wrap_key_rk(&vk, &salt).unwrap();
        let b = backup_cred_rk(&vk, &salt).unwrap();
        let c = locator_rk(&vk, &salt).unwrap();
        assert_ne!(a.expose(), b.expose());
        assert_ne!(a.expose(), c.expose());
        assert_ne!(b.expose(), c.expose());
    }

    #[test]
    fn salt_changes_output() {
        let pk = SecretBytes::new([5u8; 32]);
        let a = backup_cred_mp(&pk, &[1u8; 16]).unwrap();
        let b = backup_cred_mp(&pk, &[2u8; 16]).unwrap();
        assert_ne!(a.expose(), b.expose());
    }
}
