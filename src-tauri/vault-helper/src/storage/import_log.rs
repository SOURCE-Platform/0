//! `import_log` fingerprints (spec §10.3): `fingerprint =
//! HMAC(import_fp_key(VK), normalized_identity)`, with the identity also
//! sealed into `identity_ct` under the meta key so VK rotation can
//! recompute every fingerprint under the new generation's key inside the
//! rotation transaction (§2.10).

use rusqlite::params;

use super::header::Header;
use crate::crypto::hkdf;
use crate::crypto::record::{self, RecordCiphertext};
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;

/// Field tag for `import_log.identity_ct` under the meta key (§10.3).
pub const IMPORT_IDENTITY_TAG: &[u8] = b"import-identity";
/// `import_log` rows are vault-level: they bind the all-zero record id.
pub const IMPORT_RECORD_ID: [u8; 16] = [0u8; 16];

/// §10.3: fingerprint = HMAC(import_fp_key(VK), identity). The identity
/// is recovered from `identity_ct` (old meta key) and both columns are
/// rebuilt under the new VK, in the same transaction as the re-seal.
pub fn recompute(
    tx: &rusqlite::Transaction<'_>,
    h: &Header,
    old_vk: &SecretBytes<32>,
    new_vk: &SecretBytes<32>,
) -> Result<(), ErrorCode> {
    let rows: Vec<(Vec<u8>, Vec<u8>)> = {
        let mut stmt = tx
            .prepare("SELECT fingerprint, identity_ct FROM import_log")
            .map_err(|_| ErrorCode::DbCorrupt)?;
        let it = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|_| ErrorCode::DbCorrupt)?;
        it.collect::<Result<_, _>>().map_err(|_| ErrorCode::DbCorrupt)?
    };
    tx.execute("DELETE FROM import_log", []).map_err(|_| ErrorCode::DbCorrupt)?;
    for (_, identity_ct) in rows {
        let identity = open_identity(old_vk, h, &identity_ct)?;
        let (fp, ct) = seal_identity(new_vk, h, &identity)?;
        tx.execute(
            "INSERT INTO import_log (fingerprint, identity_ct) VALUES (?1, ?2)",
            params![fp.as_slice(), ct],
        )
        .map_err(|_| ErrorCode::DbCorrupt)?;
    }
    Ok(())
}

/// `(fingerprint, identity_ct)` for one normalized identity (§10.3).
pub fn seal_identity(
    vk: &SecretBytes<32>,
    h: &Header,
    identity: &[u8],
) -> Result<([u8; 32], Vec<u8>), ErrorCode> {
    use hmac::{Hmac, KeyInit, Mac};
    let key = hkdf::import_fp_key(vk, &h.import_fp_salt.0).map_err(|_| ErrorCode::Internal)?;
    let mut mac = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice(key.expose())
        .map_err(|_| ErrorCode::Internal)?;
    mac.update(identity);
    let fp: [u8; 32] = mac.finalize().into_bytes().into();
    let sealed = record::seal_meta_unbound(
        vk, &h.vault_id.0, &h.meta_salt.0, &IMPORT_RECORD_ID, IMPORT_IDENTITY_TAG, identity,
    )
    .map_err(|_| ErrorCode::Internal)?;
    let mut ct = sealed.nonce.to_vec();
    ct.extend_from_slice(&sealed.ct);
    Ok((fp, ct))
}

pub fn open_identity(
    vk: &SecretBytes<32>,
    h: &Header,
    identity_ct: &[u8],
) -> Result<crate::crypto::secret::SecretVec, ErrorCode> {
    if identity_ct.len() < 24 {
        return Err(ErrorCode::DbCorrupt);
    }
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&identity_ct[..24]);
    record::open_meta_unbound(
        vk,
        &h.vault_id.0,
        &h.meta_salt.0,
        &IMPORT_RECORD_ID,
        IMPORT_IDENTITY_TAG,
        &RecordCiphertext { nonce, ct: identity_ct[24..].to_vec() },
    )
    .map_err(|_| ErrorCode::RecordCorrupt)
}
