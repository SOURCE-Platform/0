//! Materializing a committed provider state on a device that is active
//! in its registry but holds no local vault (spec v0.4 §2.10 "fetches its
//! new envelope from the provider", §4.7; the Phase F multi-writer
//! rehearsals' simulated Macs). The VK comes from this device's own
//! envelope, opened inside its Enclave; the checkpoint under that VK must
//! bind the served registry head and manifest before anything is trusted.

use std::collections::HashMap;
use std::path::Path;

use sha2::{Digest, Sha256};

use super::apply::OpenEnvelope;
use super::remote::RemoteState;
use super::seen::{self, Seen};
use crate::backup::index::{ObjectIndex, Role};
use crate::backup::object;
use crate::crypto::secret::SecretBytes;
use crate::device::envelope::DeviceEnvelopeFile;
use crate::errors::ErrorCode;
use crate::registry::chain::{self, EpochPolicy};
use crate::registry::{file as registry_file, log};
use crate::storage::header::parse_header;
use crate::storage::merge::{apply_batch, NoCompare};
use crate::storage::revisions::uuid_string;
use crate::storage::store::{write_atomic, PASSWORD_WRAP_NAME, RECOVERY_WRAP_NAME};
use crate::storage::{rev_state, VaultStore};

pub fn join(
    dir: &Path,
    remote: &RemoteState,
    index: &ObjectIndex,
    blobs: &HashMap<[u8; 32], Vec<u8>>,
    me: [u8; 16],
    open_env: OpenEnvelope<'_>,
) -> Result<(VaultStore, SecretBytes<32>), ErrorCode> {
    let get = |role: &Role| -> Result<&Vec<u8>, ErrorCode> {
        let e = index.find(role).ok_or(ErrorCode::ManifestMismatch)?;
        let b = blobs.get(&e.blob).ok_or(ErrorCode::BackupObjectMissing)?;
        if <[u8; 32]>::from(Sha256::digest(b)) != e.blob {
            return Err(ErrorCode::BackupObjectMissing);
        }
        Ok(b)
    };
    if index.hash() != remote.manifest.object_index_hash {
        return Err(ErrorCode::ManifestMismatch);
    }
    let vid = remote.manifest.vault_id;
    let env: DeviceEnvelopeFile = serde_json::from_slice(get(&Role::Env { device_id: me })?).map_err(|_| ErrorCode::WrapCorrupt)?;
    let payload = open_env(&env)?;
    if payload.vk_generation != remote.vk_generation {
        return Err(ErrorCode::ManifestMismatch);
    }
    let entries = registry_file::decode(get(&Role::Registry)?)?;
    let epoch = entries.last().map_or(0, |e| e.epoch);
    remote.checkpoint.verify_binding(&payload.vk, &remote.manifest, &remote.manifest.registry_head, epoch)?;
    let reg = chain::verify_chain_with(&entries, &vid, &EpochPolicy::CheckpointAnchored)?;
    if reg.head != remote.manifest.registry_head || reg.active_device(&me).is_none() {
        return Err(ErrorCode::DeviceNotAuthorized);
    }
    let signer = reg.active_device(&remote.manifest.signer_device_id).ok_or(ErrorCode::DeviceNotAuthorized)?;
    remote.manifest.verify(&signer.sign_pub)?;
    let header = parse_header(get(&Role::Header)?)?;
    if header.vault_id.0 != vid || header.vk_generation != remote.vk_generation {
        return Err(ErrorCode::ManifestMismatch);
    }
    let mut rows = Vec::new();
    for e in index.revs() {
        let Role::Rev { record_id, revision_id, .. } = &e.role else { continue };
        let b = blobs.get(&e.blob).ok_or(ErrorCode::BackupObjectMissing)?;
        rows.push(object::decode_named(b, &e.blob, &uuid_string(record_id), revision_id)?);
    }
    std::fs::create_dir_all(dir).map_err(|_| ErrorCode::Internal)?;
    let mut store = VaultStore::create(dir, header)?;
    {
        let gen = store.header.vk_generation;
        let tx = store.conn.transaction().map_err(|_| ErrorCode::DbCorrupt)?;
        apply_batch(&tx, &rows, gen, &NoCompare)?;
        if rev_state::pending_count(&tx)? != 0 {
            return Err(ErrorCode::ManifestMismatch);
        }
        tx.commit().map_err(|_| ErrorCode::DbCorrupt)?;
    }
    store.persist_head()?;
    write_atomic(&dir.join(PASSWORD_WRAP_NAME), get(&Role::WrapMp)?)?;
    if index.find(&Role::WrapRk).is_some() {
        write_atomic(&dir.join(RECOVERY_WRAP_NAME), get(&Role::WrapRk)?)?;
    }
    for (id, e) in index.envs() {
        let bytes = blobs.get(&e.blob).ok_or(ErrorCode::BackupObjectMissing)?;
        let file: DeviceEnvelopeFile = serde_json::from_slice(bytes).map_err(|_| ErrorCode::WrapCorrupt)?;
        crate::device::envelope::write_envelope(dir, id, &file)?;
    }
    log::write_all(dir, &entries)?;
    store.set_author_device(&me)?;
    seen::save(&store.conn, &Seen::with_auth(remote.generation, remote.manifest_hash, remote.state_commit, &remote.recovery_auth))?;
    Ok((store, payload.vk))
}
