//! Device side of backup (spec §11.3 / §11.5), used by the Phase D
//! recovery engine and the FsBackupStore rehearsals: build a snapshot of
//! the local vault (record objects, header, registry, wraps, index,
//! signed manifest), upload it, and — on a recovering device — download
//! and verify one and materialize it as a local vault directory.

use std::path::Path;

use super::fs_store::{Auth, FsBackupStore};
use super::index::{self, IndexRef, ObjectIndex};
use super::manifest::SignedManifest;
use super::object;
use crate::crypto::registry::{self, RegistryEntry};
use crate::errors::ErrorCode;
use crate::registry::device::DeviceIdentity;
use crate::registry::file as registry_file;
use crate::storage::header;
use crate::storage::revision_rows;
use crate::storage::revisions::{self, RevisionRow};
use crate::storage::store::{now_epoch, write_atomic, VaultStore, PASSWORD_WRAP_NAME, RECOVERY_WRAP_NAME};
use crate::VAULT_REGISTRY_NAME;

pub struct Snapshot {
    pub objects: Vec<(String, Vec<u8>)>,
    pub index: ObjectIndex,
    pub manifest: SignedManifest,
}

/// Build (not upload) a snapshot at backup `generation`.
pub fn build(
    store: &VaultStore,
    registry_entries: &[RegistryEntry],
    generation: u64,
    prev_manifest_hash: [u8; 32],
    signer: &dyn DeviceIdentity,
) -> Result<Snapshot, ErrorCode> {
    let mut objects = Vec::new();
    let mut add = |key: String, bytes: Vec<u8>| -> IndexRef {
        let r = IndexRef::of(key.clone(), &bytes);
        objects.push((key, bytes));
        r
    };
    let mut records = Vec::new();
    for row in revision_rows::all_rows(&store.conn)? {
        records.push(add(object::key(&row), object::encode(&row)?));
    }
    records.sort_by(|a, b| a.key.cmp(&b.key));
    let header_bytes = header::write_header(&store.header)?;
    let registry_bytes = registry_file::encode(registry_entries)?;
    let mp_bytes = std::fs::read(store.dir.join(PASSWORD_WRAP_NAME)).map_err(|_| ErrorCode::WrapCorrupt)?;
    let rk_bytes = std::fs::read(store.dir.join(RECOVERY_WRAP_NAME)).ok();
    let index = ObjectIndex {
        generation,
        item_count: revision_rows::live_count(&store.conn)?,
        header: add(index::meta_key("header", &header_bytes), header_bytes),
        registry: add(index::meta_key("registry", &registry_bytes), registry_bytes),
        wrap_mp: add(index::meta_key("wrap", &mp_bytes), mp_bytes),
        wrap_rk: rk_bytes.map(|b| add(index::meta_key("wrap", &b), b)),
        records,
    };
    let registry_head = match registry_entries.last() {
        Some(e) => registry::entry_hash(e).map_err(|_| ErrorCode::Internal)?,
        None => [0u8; 32],
    };
    let manifest = SignedManifest {
        vault_id: store.header.vault_id.0,
        generation,
        created_at: now_epoch(),
        registry_head,
        vk_generation: store.header.vk_generation,
        object_index_hash: index.hash(),
        prev_manifest_hash,
        signer_device_id: [0u8; 16],
        signature: [0u8; 64],
    }
    .sign(signer)?;
    objects.push((ObjectIndex::key(generation), index.encode()));
    Ok(Snapshot { objects, index, manifest })
}

pub fn upload(backup: &FsBackupStore, vault_id: &[u8; 16], snap: &Snapshot, auth: Auth<'_>) -> Result<(), ErrorCode> {
    for (key, bytes) in &snap.objects {
        backup.put_object(vault_id, key, bytes, auth)?;
    }
    Ok(())
}

/// Build + upload + CAS publish (§11.3) by an enrolled device.
pub fn publish(
    backup: &FsBackupStore,
    store: &VaultStore,
    registry_entries: &[RegistryEntry],
    prev: Option<&SignedManifest>,
    signer: &dyn DeviceIdentity,
    auth: Auth<'_>,
) -> Result<SignedManifest, ErrorCode> {
    let (gen, prev_hash) = prev.map(|m| (m.generation, m.hash())).unwrap_or((0, [0u8; 32]));
    let snap = build(store, registry_entries, gen + 1, prev_hash, signer)?;
    let vault_id = store.header.vault_id.0;
    upload(backup, &vault_id, &snap, auth)?;
    backup.publish(&vault_id, gen, &snap.manifest.encode(), auth)?;
    Ok(snap.manifest)
}

/// A downloaded, hash-verified backup state (§11.5 steps 1–2).
pub struct Downloaded {
    pub manifest: SignedManifest,
    pub index: ObjectIndex,
    pub header_bytes: Vec<u8>,
    pub registry: Vec<RegistryEntry>,
    pub wrap_mp: Vec<u8>,
    pub wrap_rk: Option<Vec<u8>>,
    pub rows: Vec<RevisionRow>,
}

pub fn download(backup: &FsBackupStore, manifest_bytes: &[u8], auth: Auth<'_>) -> Result<Downloaded, ErrorCode> {
    let manifest = SignedManifest::decode(manifest_bytes)?;
    let vid = manifest.vault_id;
    let fetch = |r: &IndexRef| -> Result<Vec<u8>, ErrorCode> {
        let bytes = backup.get_object(&vid, &r.key, auth)?;
        index::check(r, &bytes)?;
        Ok(bytes)
    };
    let index = ObjectIndex::decode(&backup.get_object(&vid, &ObjectIndex::key(manifest.generation), auth)?)?;
    if index.hash() != manifest.object_index_hash || index.generation != manifest.generation {
        return Err(ErrorCode::ManifestMismatch);
    }
    let mut rows = Vec::with_capacity(index.records.len());
    for r in &index.records {
        let (rid, hash) = object::parse_key(&r.key)?;
        rows.push(object::decode(&rid, &hash, &fetch(r)?)?);
    }
    Ok(Downloaded {
        header_bytes: fetch(&index.header)?,
        registry: registry_file::decode(&fetch(&index.registry)?)?,
        wrap_mp: fetch(&index.wrap_mp)?,
        wrap_rk: index.wrap_rk.as_ref().map(fetch).transpose()?,
        rows,
        manifest,
        index,
    })
}

/// Write a downloaded state as a local vault directory (no VK needed:
/// ciphertext only) and open it.
pub fn materialize(dir: &Path, d: &Downloaded) -> Result<VaultStore, ErrorCode> {
    let h = header::parse_header(&d.header_bytes)?;
    if h.vault_id.0 != d.manifest.vault_id || h.vk_generation != d.manifest.vk_generation {
        return Err(ErrorCode::ManifestMismatch);
    }
    std::fs::create_dir_all(dir).map_err(|_| ErrorCode::Internal)?;
    let mut store = VaultStore::create(dir, h)?;
    store.manifest.vk_generation = store.header.vk_generation;
    store.manifest.registry_head = store.header.registry_head;
    {
        let tx = store.conn.transaction().map_err(|_| ErrorCode::DbCorrupt)?;
        for i in revision_rows::topo_order(&d.rows)? {
            revisions::apply_revision(&tx, &d.rows[i])?;
        }
        tx.commit().map_err(|_| ErrorCode::DbCorrupt)?;
    }
    store.persist_head()?;
    write_atomic(&dir.join(PASSWORD_WRAP_NAME), &d.wrap_mp)?;
    if let Some(rk) = &d.wrap_rk {
        write_atomic(&dir.join(RECOVERY_WRAP_NAME), rk)?;
    }
    write_atomic(&dir.join(VAULT_REGISTRY_NAME), &registry_file::encode(&d.registry)?)?;
    Ok(store)
}
