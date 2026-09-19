//! Recovery-class locators and credentials (spec §11.4, §12 scenario 3):
//!
//! ```text
//! locator_mp = HMAC-SHA256(HKDF(PK, salt=locator_salt_mp, "ov0/locate/mp/v1"), "ov0/locator/v1")
//! cred_mp    = HKDF-SHA256(PK, salt=locator_salt_mp, "ov0/backup-auth/mp/v1")
//! ```
//! and the RK analogues. Never persisted on any client (CR-13); they
//! exist only transiently in the helper.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::crypto::hkdf;
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;

pub struct RecoveryCreds {
    pub locator: [u8; 32],
    pub cred: SecretBytes<32>,
}

fn locator_value(key: &SecretBytes<32>) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key.expose()).expect("any key length");
    mac.update(b"ov0/locator/v1");
    mac.finalize().into_bytes().into()
}

pub fn mp_creds(pk: &SecretBytes<32>, locator_salt_mp: &[u8; 16]) -> Result<RecoveryCreds, ErrorCode> {
    let key = hkdf::locator_mp(pk, locator_salt_mp).map_err(|_| ErrorCode::Internal)?;
    Ok(RecoveryCreds {
        locator: locator_value(&key),
        cred: hkdf::backup_cred_mp(pk, locator_salt_mp).map_err(|_| ErrorCode::Internal)?,
    })
}

pub fn rk_creds(rk: &SecretBytes<32>, locator_salt_rk: &[u8; 16]) -> Result<RecoveryCreds, ErrorCode> {
    let key = hkdf::locator_rk(rk, locator_salt_rk).map_err(|_| ErrorCode::Internal)?;
    Ok(RecoveryCreds {
        locator: locator_value(&key),
        cred: hkdf::backup_cred_rk(rk, locator_salt_rk).map_err(|_| ErrorCode::Internal)?,
    })
}
