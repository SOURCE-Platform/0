//! VK rotation engine (spec §2.10): new VK → `vk_generation + 1` → every
//! revision re-sealed → every wrap rewritten → every `import_log`
//! fingerprint recomputed → new manifest generation → old VK dropped.
//!
//! v0.4: revisions keep their `revision_id`, parents, author and counter
//! (§3.2); only ciphertexts, nonces and `vk_generation` change, so the
//! logical graph is never renamed and heads/conflicts need no remap.
//!
//! Everything is staged beside the live vault and committed through
//! `rotation_journal` — a crash leaves the vault entirely old or entirely
//! new, never half-rotated.

use rusqlite::{params, Connection};

use super::db::DB_NAME;
use super::header::{self, Header};
use super::manifest::{self, ManifestObject, MANIFEST_NAME};
use super::revision_rows;
use super::revisions;
use super::rotation_journal::{self as journal, CommitMarker, FailAt};
use super::store::{write_atomic, VaultStore, PASSWORD_WRAP_NAME, RECOVERY_WRAP_NAME};
use crate::crypto::kdf;
use crate::crypto::record::{self, RecordCiphertext};
use super::revisions::insert_rev;
use crate::crypto::secret::{random_secret, SecretBytes};
use crate::crypto::wrap::{self, RecoveryWrapPayload};
use crate::errors::ErrorCode;
use crate::VAULT_HEADER_NAME;

/// How the MP wrap is rebuilt under the new VK.
pub enum MpWrap<'a> {
    /// Same MP: re-seal with the existing file's KDF salt/params, using
    /// the PK the caller derived (and proved) from the entered MP.
    Reseal(&'a SecretBytes<32>),
    /// A newly set MP (e.g. RK recovery): fresh salt, v1 params.
    Fresh(&'a [u8]),
}

/// How the RK wrap is rebuilt under the new VK.
pub enum RkWrap<'a> {
    /// Seal for this RK (kept or newly generated).
    Seal(&'a SecretBytes<32>),
    /// Drop recovery.wrap (caller cannot re-seal it; a wrap of the old VK
    /// must not survive the rotation).
    Remove,
}

/// Files a rotation must re-seal that the storage layer does not own:
/// the per-device HPKE envelopes and the credential store (§2.2, §11.4).
/// Implemented in `device::rotate`; staged under the same journal so the
/// new VK and the new envelopes commit together.
pub trait ExtraStaging {
    fn stage(
        &self,
        dir: &std::path::Path,
        new_vk: &SecretBytes<32>,
        new_vk_generation: u32,
    ) -> Result<Vec<String>, ErrorCode>;
}

pub struct RotationOutcome {
    pub new_vk: SecretBytes<32>,
    pub vk_generation: u32,
    pub manifest_generation: u64,
}

/// Rotate the open vault. Consumes the store (the live DB must be closed
/// before commit) and returns the new VK plus the new generations; the
/// caller reopens the vault.
pub fn rotate(
    store: VaultStore,
    old_vk: &SecretBytes<32>,
    mp: MpWrap<'_>,
    rk: RkWrap<'_>,
    extra: Option<&dyn ExtraStaging>,
    fail: Option<FailAt>,
) -> Result<RotationOutcome, ErrorCode> {
    let dir = store.dir.clone();
    journal::discard_staged(&dir);
    let old = store.header.clone();
    let new_vk = random_secret();
    let new_gen = old.vk_generation.checked_add(1).ok_or(ErrorCode::Internal)?;

    // 1. Stage the DB: a consistent copy, rewritten under the new VK.
    let next_db = journal::next_path(&dir, DB_NAME);
    let next_db_str = next_db.to_str().ok_or(ErrorCode::Internal)?;
    store
        .conn
        .execute("VACUUM INTO ?1", params![next_db_str])
        .map_err(|_| ErrorCode::DbCorrupt)?;
    let mut next = Connection::open(&next_db).map_err(|_| ErrorCode::DbCorrupt)?;
    next.pragma_update(None, "journal_mode", "DELETE")
        .map_err(|_| ErrorCode::DbCorrupt)?;
    let objects = reseal_db(&mut next, &old, old_vk, &new_vk, new_gen)?;
    let item_count = revision_rows::live_count(&next)?;
    drop(next);
    if fail == Some(FailAt::AfterDbStaged) {
        return Err(ErrorCode::Internal);
    }

    // 2. Stage the wraps.
    let mut new_header = old.clone();
    stage_wraps(&dir, &mut new_header, &new_vk, new_gen, mp, &rk)?;
    let staged_extra = match extra {
        Some(e) => e.stage(&dir, &new_vk, new_gen)?,
        None => Vec::new(),
    };
    if fail == Some(FailAt::AfterWrapsStaged) {
        return Err(ErrorCode::Internal);
    }

    // 3. Stage header + manifest (the accepted pair).
    new_header.vk_generation = new_gen;
    new_header.manifest_generation = old.manifest_generation + 1;
    let mut new_manifest = store.manifest.clone();
    new_manifest.vk_generation = new_gen;
    new_manifest.manifest_generation = new_header.manifest_generation;
    new_manifest.objects = objects;
    new_manifest.item_count = item_count;
    write_atomic(
        &journal::next_path(&dir, VAULT_HEADER_NAME),
        &header::write_header(&new_header)?,
    )?;
    write_atomic(
        &journal::next_path(&dir, MANIFEST_NAME),
        &manifest::write_manifest(&new_manifest)?,
    )?;

    // 4. Close the live DB with an empty WAL, then commit.
    let _ = store
        .conn
        .pragma_update(None, "wal_checkpoint", "TRUNCATE");
    drop(store);
    let marker = CommitMarker {
        new_vk_generation: new_gen,
        new_manifest_generation: new_header.manifest_generation,
        remove: match rk {
            RkWrap::Remove => vec![RECOVERY_WRAP_NAME.to_string()],
            RkWrap::Seal(_) => Vec::new(),
        },
        stage: staged_extra,
    };
    journal::commit(&dir, &marker, fail)?;
    Ok(RotationOutcome {
        new_vk,
        vk_generation: new_gen,
        manifest_generation: new_header.manifest_generation,
    })
}

/// Re-seal every revision and every import fingerprint inside one
/// transaction of the staged DB. Returns the new manifest object list.
fn reseal_db(
    conn: &mut Connection,
    h: &Header,
    old_vk: &SecretBytes<32>,
    new_vk: &SecretBytes<32>,
    new_gen: u32,
) -> Result<Vec<ManifestObject>, ErrorCode> {
    let tx = conn.transaction().map_err(|_| ErrorCode::DbCorrupt)?;
    let rows = revision_rows::all_rows(&tx)?;
    let mut objects = Vec::with_capacity(rows.len());
    for mut row in rows {
        let rid = revisions::uuid_bytes(&row.record_id).ok_or(ErrorCode::DbCorrupt)?;
        let bind = row.bind()?;
        let vid = &h.vault_id.0;
        let pt = record::open_record(
            old_vk,
            vid,
            &rid,
            &bind,
            row.schema_version,
            row.vk_generation,
            &RecordCiphertext { nonce: row.nonce, ct: row.ct.clone() },
        )
        .map_err(|_| ErrorCode::RecordCorrupt)?;
        let meta = record::open_meta(
            old_vk,
            vid,
            &h.meta_salt.0,
            &rid,
            &bind,
            super::store::META_FIELD_TAG,
            &RecordCiphertext { nonce: row.meta_nonce, ct: row.meta_ct.clone() },
        )
        .map_err(|_| ErrorCode::RecordCorrupt)?;
        let ct = record::seal_record(new_vk, vid, &rid, &bind, row.schema_version, new_gen, &pt)
            .map_err(|_| ErrorCode::Internal)?;
        let meta_ct = record::seal_meta(new_vk, vid, &h.meta_salt.0, &rid, &bind, super::store::META_FIELD_TAG, &meta)
            .map_err(|_| ErrorCode::Internal)?;
        row.vk_generation = new_gen;
        (row.nonce, row.ct, row.meta_nonce, row.meta_ct) = (ct.nonce, ct.ct, meta_ct.nonce, meta_ct.ct);
        insert_rev(&tx, &row)?; // same revision_id: replaced in place
        objects.push(ManifestObject {
            record_id: row.record_id.clone(),
            revision_id: crate::crypto::hex::encode(row.revision_id),
        });
    }
    super::import_log::recompute(&tx, h, old_vk, new_vk)?;
    // Held-back revisions are sealed under the retiring VK and are not
    // this vault's to re-seal; the next committed state brings them back.
    super::rev_state::purge_pending(&tx)?;
    tx.commit().map_err(|_| ErrorCode::DbCorrupt)?;
    Ok(objects)
}

fn stage_wraps(
    dir: &std::path::Path,
    new_header: &mut Header,
    new_vk: &SecretBytes<32>,
    new_gen: u32,
    mp: MpWrap<'_>,
    rk: &RkWrap<'_>,
) -> Result<(), ErrorCode> {
    let payload = || RecoveryWrapPayload {
        vk: SecretBytes::new(*new_vk.expose()),
        wrapped_at: super::store::now_epoch(),
        vk_generation: new_gen,
    };
    let vault_id = new_header.vault_id.0;
    let mp_file = match mp {
        MpWrap::Reseal(pk) => {
            let bytes = std::fs::read(dir.join(PASSWORD_WRAP_NAME)).map_err(|_| ErrorCode::WrapCorrupt)?;
            let current: wrap::PasswordWrapFile =
                serde_json::from_slice(&bytes).map_err(|_| ErrorCode::WrapCorrupt)?;
            // Prove the PK against the live wrap before trusting it.
            wrap::open_wrap_mp(&current, pk, &vault_id).map_err(|_| ErrorCode::WrongCredential)?;
            crate::crypto::rotate::rotate_wrap_mp(&current, pk, &vault_id, new_vk, super::store::now_epoch(), new_gen)
                .map_err(|_| ErrorCode::Internal)?
        }
        MpWrap::Fresh(mp) => {
            let salt: [u8; 16] = crate::crypto::secret::random_salt();
            let params = kdf::Argon2Params::V1;
            let pk = kdf::derive_pk(mp, &salt, params).map_err(|_| ErrorCode::Internal)?;
            let file = wrap::seal_wrap_mp(&payload(), &pk, &vault_id, params, &salt)
                .map_err(|_| ErrorCode::Internal)?;
            new_header.kdf = header::KdfBlock {
                salt: crate::crypto::hex::encode(salt),
                ..header::KdfBlock::v1()
            };
            file
        }
    };
    write_atomic(
        &journal::next_path(dir, PASSWORD_WRAP_NAME),
        &serde_json::to_vec_pretty(&mp_file).map_err(|_| ErrorCode::Internal)?,
    )?;
    if let RkWrap::Seal(rk) = rk {
        let file = wrap::seal_wrap_rk(&payload(), rk, &vault_id).map_err(|_| ErrorCode::Internal)?;
        write_atomic(
            &journal::next_path(dir, RECOVERY_WRAP_NAME),
            &serde_json::to_vec_pretty(&file).map_err(|_| ErrorCode::Internal)?,
        )?;
    }
    Ok(())
}
