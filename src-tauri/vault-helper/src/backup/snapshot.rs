//! Device side of backup (spec §11.3 / §11.5), used by the Phase D
//! recovery engine and the FsBackupStore rehearsals: build a snapshot of
//! the local vault (record objects, header, registry, wraps, index,
//! signed manifest), upload it, and — on a recovering device — download
//! and verify one and materialize it as a local vault directory.

use std::path::Path;

use super::checkpoint::RegistryCheckpoint;
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
use crate::storage::revisions::RevisionRow;
use crate::storage::store::{now_epoch, write_atomic, VaultStore, PASSWORD_WRAP_NAME, RECOVERY_WRAP_NAME};
use crate::VAULT_REGISTRY_NAME;

pub struct Snapshot {
    pub objects: Vec<(String, Vec<u8>)>,
    pub index: ObjectIndex,
    pub manifest: SignedManifest,
    /// §4.8: MAC'd under the current VK; its own object, never in the
    /// index (that would make the hashes recursive).
    pub checkpoint: RegistryCheckpoint,
}

/// Build (not upload) a snapshot at backup `generation`.
pub fn build(
    store: &VaultStore,
    registry_entries: &[RegistryEntry],
    generation: u64,
    prev_manifest_hash: [u8; 32],
    signer: &dyn DeviceIdentity,
    vk: &crate::crypto::secret::SecretBytes<32>,
) -> Result<Snapshot, ErrorCode> {
    let mut objects = Vec::new();
    let mut add = |key: String, bytes: Vec<u8>| -> IndexRef {
        let r = IndexRef::of(key.clone(), &bytes);
        objects.push((key, bytes));
        r
    };
    let mut records = Vec::new();
    for row in revision_rows::all_rows(&store.conn)? {
        let bytes = object::encode(&row)?;
        records.push(add(record_key(&row, &object::blob_hash(&bytes)), bytes));
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
    let epoch = registry_entries.last().map_or(0, |e| e.epoch);
    let checkpoint = RegistryCheckpoint::create(vk, &manifest, epoch)?;
    objects.push((RegistryCheckpoint::key(generation), checkpoint.encode()));
    Ok(Snapshot { objects, index, manifest, checkpoint })
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
    vk: &crate::crypto::secret::SecretBytes<32>,
    auth: Auth<'_>,
) -> Result<SignedManifest, ErrorCode> {
    let (gen, prev_hash) = prev.map(|m| (m.generation, m.hash())).unwrap_or((0, [0u8; 32]));
    let snap = build(store, registry_entries, gen + 1, prev_hash, signer, vk)?;
    let vault_id = store.header.vault_id.0;
    upload(backup, &vault_id, &snap, auth)?;
    backup.publish(&vault_id, gen, &snap.manifest.encode(), &snap.checkpoint.encode(), auth)?;
    Ok(snap.manifest)
}

/// A downloaded, hash-verified backup state (§11.5 steps 1–2).
pub struct Downloaded {
    pub manifest: SignedManifest,
    /// The §4.8 checkpoint served with this state (verified by the caller
    /// against the recovered VK before the registry is trusted).
    pub checkpoint: RegistryCheckpoint,
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
    let checkpoint = RegistryCheckpoint::decode(
        &backup.get_object(&vid, &RegistryCheckpoint::key(manifest.generation), auth)?,
    )?;
    let index = ObjectIndex::decode(&backup.get_object(&vid, &ObjectIndex::key(manifest.generation), auth)?)?;
    if index.hash() != manifest.object_index_hash || index.generation != manifest.generation {
        return Err(ErrorCode::ManifestMismatch);
    }
    let mut rows = Vec::with_capacity(index.records.len());
    for r in &index.records {
        let (rid, id) = parse_record_key(&r.key)?;
        let bytes = fetch(r)?;
        let row = object::decode(&bytes)?;
        if row.record_id != rid || row.revision_id != id {
            return Err(ErrorCode::ManifestMismatch);
        }
        rows.push(row);
    }
    Ok(Downloaded {
        header_bytes: fetch(&index.header)?,
        registry: registry_file::decode(&fetch(&index.registry)?)?,
        wrap_mp: fetch(&index.wrap_mp)?,
        wrap_rk: index.wrap_rk.as_ref().map(fetch).transpose()?,
        rows,
        manifest,
        checkpoint,
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
        crate::storage::merge::apply_batch(&tx, &d.rows, &crate::storage::merge::NoCompare)?;
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

/// Interim snapshot key for a record object (superseded by the v2 index
/// later in Phase F): `objects/rec/<record_id>/<revision_id>/<blob_hash>`.
/// The blob hash is part of the name (§3.7): a rotation re-seal is a new
/// blob and never overwrites the one a retained manifest points at.
pub fn record_key(row: &RevisionRow, blob_hash: &[u8; 32]) -> String {
    let hex = crate::crypto::hex::encode;
    format!("objects/rec/{}/{}/{}", row.record_id, hex(row.revision_id), hex(*blob_hash))
}

fn parse_record_key(key: &str) -> Result<(String, [u8; 32]), ErrorCode> {
    let rest = key.strip_prefix("objects/rec/").ok_or(ErrorCode::BackupObjectMissing)?;
    let mut parts = rest.split('/');
    let (Some(rid), Some(id), Some(_blob), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(ErrorCode::BackupObjectMissing);
    };
    let id = crate::crypto::hex::decode_array::<32>(id).ok_or(ErrorCode::BackupObjectMissing)?;
    Ok((rid.to_string(), id))
}
