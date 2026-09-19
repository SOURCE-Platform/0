//! VK rotation engine (spec §2.10): new VK → `vk_generation + 1` → every
//! revision re-sealed → every wrap rewritten → every `import_log`
//! fingerprint recomputed → new manifest generation → old VK dropped.
//!
//! Content-committed revision hashes (§3.2) include SHA-256 of the
//! ciphertext, so re-sealing changes every `rev_hash`. The engine
//! re-derives them deterministically in topological order and remaps
//! `parent_revs`, `record_tips`, and `record_conflicts` to the new hashes,
//! so the rotated graph has exactly the old shape. (Spec clarification,
//! documented Phase D deviation: §2.10 does not mention the remap.)
//!
//! Everything is staged beside the live vault and committed through
//! `rotation_journal` — a crash leaves the vault entirely old or entirely
//! new, never half-rotated.

use std::collections::HashMap;

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

pub struct RotationOutcome {
    pub new_vk: SecretBytes<32>,
    pub vk_generation: u32,
    pub manifest_generation: u64,
}

struct Rotated {
    old_hash: [u8; 32],
    row: revisions::RevisionRow,
}

/// Rotate the open vault. Consumes the store (the live DB must be closed
/// before commit) and returns the new VK plus the new generations; the
/// caller reopens the vault.
pub fn rotate(
    store: VaultStore,
    old_vk: &SecretBytes<32>,
    mp: MpWrap<'_>,
    rk: RkWrap<'_>,
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
    let order = revision_rows::topo_order(&rows)?;
    let mut remap: HashMap<[u8; 32], [u8; 32]> = HashMap::new();
    let mut rotated = Vec::with_capacity(rows.len());
    for idx in order {
        let mut row = rows[idx].clone();
        let rid = revisions::uuid_bytes(&row.record_id).ok_or(ErrorCode::DbCorrupt)?;
        let dev = revisions::uuid_bytes(&row.author_device).ok_or(ErrorCode::DbCorrupt)?;
        let pt = record::open_record(
            old_vk,
            &h.vault_id.0,
            &rid,
            row.schema_version,
            row.vk_generation,
            &RecordCiphertext { nonce: row.nonce, ct: row.ct.clone() },
        )
        .map_err(|_| ErrorCode::RecordCorrupt)?;
        let meta = record::open_meta(
            old_vk,
            &h.vault_id.0,
            &h.meta_salt.0,
            &rid,
            super::store::META_FIELD_TAG,
            &RecordCiphertext { nonce: row.meta_nonce, ct: row.meta_ct.clone() },
        )
        .map_err(|_| ErrorCode::RecordCorrupt)?;
        let ct = record::seal_record(new_vk, &h.vault_id.0, &rid, row.schema_version, new_gen, &pt)
            .map_err(|_| ErrorCode::Internal)?;
        let meta_ct = record::seal_meta(
            new_vk,
            &h.vault_id.0,
            &h.meta_salt.0,
            &rid,
            super::store::META_FIELD_TAG,
            &meta,
        )
        .map_err(|_| ErrorCode::Internal)?;
        let old_hash = row.rev_hash;
        row.parent_revs = row
            .parent_revs
            .iter()
            .map(|p| remap.get(p).copied().ok_or(ErrorCode::DbCorrupt))
            .collect::<Result<_, _>>()?;
        row.rev_hash = revisions::rev_hash(
            &rid, &row.parent_revs, &dev, row.counter, row.deleted, &ct.ct, &meta_ct.ct,
        );
        row.vk_generation = new_gen;
        row.nonce = ct.nonce;
        row.ct = ct.ct;
        row.meta_nonce = meta_ct.nonce;
        row.meta_ct = meta_ct.ct;
        remap.insert(old_hash, row.rev_hash);
        rotated.push(Rotated { old_hash, row });
    }
    for r in &rotated {
        revision_rows::replace_row(&tx, &r.old_hash, &r.row)?;
    }
    for (old_hash, new_hash) in &remap {
        tx.execute(
            "UPDATE record_tips SET tip_rev=?2 WHERE tip_rev=?1",
            params![old_hash.as_slice(), new_hash.as_slice()],
        )
        .map_err(|_| ErrorCode::DbCorrupt)?;
        tx.execute(
            "UPDATE record_conflicts SET rev_hash=?2 WHERE rev_hash=?1",
            params![old_hash.as_slice(), new_hash.as_slice()],
        )
        .map_err(|_| ErrorCode::DbCorrupt)?;
    }
    super::import_log::recompute(&tx, h, old_vk, new_vk)?;
    tx.commit().map_err(|_| ErrorCode::DbCorrupt)?;
    Ok(rotated
        .into_iter()
        .map(|r| ManifestObject {
            record_id: r.row.record_id,
            rev_hash: crate::crypto::hex::encode(r.row.rev_hash),
        })
        .collect())
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
