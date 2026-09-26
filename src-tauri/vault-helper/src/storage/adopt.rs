//! Adopting a committed provider state's vault-wide singletons (spec v0.4
//! §11.3 "Vault-wide singletons", §2.10): the header, both wraps, the
//! active devices' envelopes and the registry — and, when another device
//! rotated the VK, every local revision re-sealed under the adopted VK
//! with the same `revision_id`s. One §2.10 journaled commit: a crash
//! leaves the pre-adoption or the post-adoption vault, never a mix.

use rusqlite::{params, Connection};

use super::db::DB_NAME;
use super::header::{self, Header};
use super::manifest::{self, ManifestObject, MANIFEST_NAME};
use super::revision_rows;
use super::rotation::reseal_db;
use super::rotation_journal::{self as journal, CommitMarker};
use super::store::{write_atomic, VaultStore, PASSWORD_WRAP_NAME, RECOVERY_WRAP_NAME};
use crate::crypto::hex;
use crate::crypto::secret::SecretBytes;
use crate::device::envelope::{self, DEVICES_DIR};
use crate::errors::ErrorCode;
use crate::{VAULT_HEADER_NAME, VAULT_REGISTRY_NAME};

pub struct Adoption {
    /// The committed header (its `manifest_generation` is replaced by the
    /// local flip).
    pub header: Header,
    pub wrap_mp: Vec<u8>,
    pub wrap_rk: Option<Vec<u8>>,
    /// Exactly the active devices' envelopes.
    pub envelopes: Vec<([u8; 16], Vec<u8>)>,
    pub registry: Vec<u8>,
}

/// `reseal = Some((old_vk, adopted_vk))` when the committed state is at a
/// newer `vk_generation`. Consumes the store; the caller reopens it.
pub fn adopt(store: VaultStore, a: &Adoption, reseal: Option<(&SecretBytes<32>, &SecretBytes<32>)>) -> Result<(), ErrorCode> {
    let dir = store.dir.clone();
    journal::discard_staged(&dir);
    let old = store.header.clone();
    let objects = match reseal {
        Some((old_vk, new_vk)) => {
            let next_db = journal::next_path(&dir, DB_NAME);
            let path = next_db.to_str().ok_or(ErrorCode::Internal)?;
            store.conn.execute("VACUUM INTO ?1", params![path]).map_err(|_| ErrorCode::DbCorrupt)?;
            let mut next = Connection::open(&next_db).map_err(|_| ErrorCode::DbCorrupt)?;
            next.pragma_update(None, "journal_mode", "DELETE").map_err(|_| ErrorCode::DbCorrupt)?;
            let objects = reseal_db(&mut next, &old, old_vk, new_vk, a.header.vk_generation)?;
            drop(next);
            objects
        }
        None => revision_rows::all_rows(&store.conn)?
            .iter()
            .map(|r| ManifestObject { record_id: r.record_id.clone(), revision_id: hex::encode(r.revision_id) })
            .collect(),
    };
    let item_count = revision_rows::live_count(&store.conn)?;
    write_atomic(&journal::next_path(&dir, PASSWORD_WRAP_NAME), &a.wrap_mp)?;
    if let Some(rk) = &a.wrap_rk {
        write_atomic(&journal::next_path(&dir, RECOVERY_WRAP_NAME), rk)?;
    }
    let mut stage = Vec::new();
    for (id, bytes) in &a.envelopes {
        let name = format!("wraps/{DEVICES_DIR}/{}.wrap", hex::encode(id));
        let next = journal::next_path(&dir, &name);
        std::fs::create_dir_all(next.parent().ok_or(ErrorCode::Internal)?).map_err(|_| ErrorCode::Internal)?;
        write_atomic(&next, bytes)?;
        stage.push(name);
    }
    write_atomic(&journal::next_path(&dir, VAULT_REGISTRY_NAME), &a.registry)?;
    stage.push(VAULT_REGISTRY_NAME.to_string());
    let mut remove: Vec<String> = envelope::list_envelopes(&dir)
        .iter()
        .filter(|id| !a.envelopes.iter().any(|(e, _)| e == *id))
        .map(|id| format!("wraps/{DEVICES_DIR}/{}.wrap", hex::encode(id)))
        .collect();
    if a.wrap_rk.is_none() {
        remove.push(RECOVERY_WRAP_NAME.to_string());
    }
    let mut new_header = a.header.clone();
    new_header.manifest_generation = old.manifest_generation + 1;
    let mut new_manifest = store.manifest.clone();
    new_manifest.vk_generation = new_header.vk_generation;
    new_manifest.registry_head = new_header.registry_head;
    new_manifest.manifest_generation = new_header.manifest_generation;
    new_manifest.objects = objects;
    new_manifest.item_count = item_count;
    write_atomic(&journal::next_path(&dir, VAULT_HEADER_NAME), &header::write_header(&new_header)?)?;
    write_atomic(&journal::next_path(&dir, MANIFEST_NAME), &manifest::write_manifest(&new_manifest)?)?;
    let _ = store.conn.pragma_update(None, "wal_checkpoint", "TRUNCATE");
    drop(store);
    journal::commit(
        &dir,
        &CommitMarker {
            new_vk_generation: new_header.vk_generation,
            new_manifest_generation: new_header.manifest_generation,
            remove,
            stage,
        },
        None,
    )
}

/// A singleton change that keeps the VK (an MP change, §11.3.2): the new
/// `password.wrap` and header plus the DB-side records `db` writes (the
/// `pending_remote` component), in one journaled commit — the MP state is
/// old or new with its pending record, never a mix. Consumes the store.
pub fn commit_singleton_change(
    store: VaultStore,
    header: &Header,
    wrap_mp: &[u8],
    db: &dyn Fn(&Connection) -> Result<(), ErrorCode>,
) -> Result<(), ErrorCode> {
    let dir = store.dir.clone();
    journal::discard_staged(&dir);
    let next_db = journal::next_path(&dir, DB_NAME);
    let path = next_db.to_str().ok_or(ErrorCode::Internal)?;
    store.conn.execute("VACUUM INTO ?1", params![path]).map_err(|_| ErrorCode::DbCorrupt)?;
    {
        let next = Connection::open(&next_db).map_err(|_| ErrorCode::DbCorrupt)?;
        next.pragma_update(None, "journal_mode", "DELETE").map_err(|_| ErrorCode::DbCorrupt)?;
        db(&next)?;
    }
    write_atomic(&journal::next_path(&dir, PASSWORD_WRAP_NAME), wrap_mp)?;
    let mut h = header.clone();
    h.manifest_generation = store.header.manifest_generation + 1;
    let mut m = store.manifest.clone();
    m.manifest_generation = h.manifest_generation;
    write_atomic(&journal::next_path(&dir, VAULT_HEADER_NAME), &header::write_header(&h)?)?;
    write_atomic(&journal::next_path(&dir, MANIFEST_NAME), &manifest::write_manifest(&m)?)?;
    let _ = store.conn.pragma_update(None, "wal_checkpoint", "TRUNCATE");
    drop(store);
    journal::commit(
        &dir,
        &CommitMarker { new_vk_generation: h.vk_generation, new_manifest_generation: h.manifest_generation, remove: Vec::new(), stage: Vec::new() },
        None,
    )
}
