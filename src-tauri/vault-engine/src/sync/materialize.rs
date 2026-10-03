//! First materialization on a joining phone (spec v0.5 §22.10, replacing
//! §4.8's bundle order and §5.1's last step for the engine). The bundle
//! arrived over TLS pinned to the QR's certificate and its SAS was
//! compared on both screens; that channel is the anchor. In this order,
//! before anything is written:
//!
//! 1. the registry chain from the bundle (§4.4), whose head is the one the
//!    phone must ACK and whose last entry is the Mac named by the QR
//!    enrolling exactly this device's keys under the id it assigned;
//! 2. the manifest's signature under a device active in that registry;
//! 3. this device's own envelope (Face ID, §22.4), bound to the nonce the
//!    Mac sent in its hello reply;
//! 4. the checkpoint under that VK, bound to the manifest and the head;
//!
//! then the index's revisions, each recorded with source `provider`
//! (§22.7). The test-only `join.rs` is not used.

use std::collections::HashMap;
use std::path::Path;

use sha2::{Digest, Sha256};

use super::apply::OpenEnvelope;
use crate::backup::checkpoint::RegistryCheckpoint;
use crate::backup::index::{ObjectIndex, Role};
use crate::backup::manifest::SignedManifest;
use crate::backup::object;
use crate::crypto::hex;
use crate::crypto::secret::SecretBytes;
use crate::device::envelope::{self, DeviceEnvelopeFile};
use crate::enroll::wire::Bundle;
use crate::errors::ErrorCode;
use crate::registry::chain::{verify_chain_with, EpochPolicy};
use crate::registry::device::DeviceIdentity;
use crate::registry::{file as registry_file, log};
use crate::storage::header::parse_header;
use crate::storage::merge::{apply_batch, NoCompare};
use crate::storage::revisions::uuid_string;
use crate::storage::sources::{self, Source};
use crate::storage::store::{write_atomic, PASSWORD_WRAP_NAME, RECOVERY_WRAP_NAME};
use crate::storage::{kv, rev_state, VaultStore};
use vault_proto::crypto::registry::EntryKind;

/// What the pinned channel established before the bundle arrived.
pub struct Anchor<'a> {
    pub me: &'a dyn DeviceIdentity,
    pub mac_device_id: [u8; 16],
    pub vault_id: [u8; 16],
    pub nonce_e: [u8; 16],
}

/// The provider state the authorizing Mac had accepted: the phone never
/// accepts an older one (§22.10 provider floor), until it has accepted
/// one of its own (`fetch::offer`).
pub const JOIN_FLOOR_KEY: &str = "join_provider_floor";

fn bad() -> ErrorCode {
    ErrorCode::ManifestMismatch
}

/// Verify the bundle in the §22.10 order and build the vault in `dir`.
/// Returns the store, the VK and the registry head to ACK.
pub fn materialize(dir: &Path, bundle: &Bundle, a: &Anchor<'_>, open_env: OpenEnvelope<'_>) -> Result<(VaultStore, SecretBytes<32>, [u8; 32]), ErrorCode> {
    let vid = hex::decode_array::<16>(&bundle.vault_id).ok_or_else(bad)?;
    if vid != a.vault_id {
        return Err(ErrorCode::ProtocolViolation);
    }
    // 1. The registry, anchored on the channel.
    let entries = registry_file::decode(&hex::decode(&bundle.registry).ok_or_else(bad)?)?;
    let reg = verify_chain_with(&entries, &vid, &EpochPolicy::CheckpointAnchored)?;
    let head = hex::decode_array::<32>(&bundle.registry_head).ok_or_else(bad)?;
    let last = entries.last().ok_or_else(bad)?;
    let mine = last.kind == EntryKind::Enroll
        && last.device_id == a.me.device_id()
        && last.sign_pub == Some(a.me.sign_pub())
        && last.agree_pub == Some(a.me.agree_pub())
        && last.authorizer == Some(a.mac_device_id);
    if reg.head != head || !mine || reg.active_device(&a.mac_device_id).is_none() {
        return Err(ErrorCode::DeviceNotAuthorized);
    }
    // 2. The manifest, signed by a device active in it.
    let manifest = SignedManifest::decode(&hex::decode(&bundle.manifest).ok_or_else(bad)?)?;
    let signer = reg.active_device(&manifest.signer_device_id).ok_or(ErrorCode::DeviceNotAuthorized)?;
    manifest.verify(&signer.sign_pub)?;
    if manifest.vault_id != vid || manifest.registry_head != head {
        return Err(bad());
    }
    // 3. This device's own envelope, for this enrollment.
    let env: DeviceEnvelopeFile = serde_json::from_value(bundle.envelope.clone()).map_err(|_| ErrorCode::WrapCorrupt)?;
    if env.device_id != hex::encode(a.me.device_id()) || env.enrollment_nonce != hex::encode(a.nonce_e) {
        return Err(ErrorCode::WrapCorrupt);
    }
    let payload = open_env(&env)?;
    if payload.vk_generation != manifest.vk_generation {
        return Err(bad());
    }
    // 4. The checkpoint under that VK.
    let checkpoint = RegistryCheckpoint::decode(&hex::decode(&bundle.checkpoint).ok_or_else(bad)?)?;
    let epoch = entries.last().map_or(0, |e| e.epoch);
    checkpoint.verify_binding(&payload.vk, &manifest, &head, epoch)?;

    // The objects, each under its own hash, and the index the manifest names.
    let mut blobs: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
    for (k, v) in &bundle.objects {
        let (Some(k), Some(v)) = (hex::decode_array::<32>(k), hex::decode(v)) else { return Err(bad()) };
        if <[u8; 32]>::from(Sha256::digest(&v)) != k {
            return Err(ErrorCode::BackupObjectMissing);
        }
        blobs.insert(k, v);
    }
    let index = ObjectIndex::decode(blobs.get(&manifest.object_index_hash).ok_or(ErrorCode::BackupObjectMissing)?).map_err(|_| bad())?;
    if index.hash() != manifest.object_index_hash || index.generation != manifest.generation {
        return Err(bad());
    }
    index.check_structure().map_err(|_| bad())?;
    let get = |role: &Role| -> Result<&Vec<u8>, ErrorCode> {
        let e = index.find(role).ok_or_else(bad)?;
        blobs.get(&e.blob).ok_or(ErrorCode::BackupObjectMissing)
    };
    let mut header = parse_header(get(&Role::Header)?)?;
    if header.vault_id.0 != vid || header.vk_generation != manifest.vk_generation {
        return Err(bad());
    }
    // The Mac's header predates the entry enrolling this phone (it is
    // appended only at the ACK); this device's copy starts at that head.
    header.registry_head = crate::storage::header::Hex32(head);
    let mut rows = Vec::new();
    for e in index.revs() {
        let Role::Rev { record_id, revision_id, .. } = &e.role else { continue };
        let b = blobs.get(&e.blob).ok_or(ErrorCode::BackupObjectMissing)?;
        rows.push(object::decode_named(b, &e.blob, &uuid_string(record_id), revision_id)?);
    }

    // Everything verified: write the vault.
    std::fs::create_dir_all(dir).map_err(|_| ErrorCode::Internal)?;
    let mut store = VaultStore::create(dir, header)?;
    {
        let gen = store.header.vk_generation;
        let tx = store.conn.transaction().map_err(|_| ErrorCode::DbCorrupt)?;
        apply_batch(&tx, &rows, gen, &NoCompare)?;
        if rev_state::pending_count(&tx)? != 0 {
            return Err(bad());
        }
        for r in &rows {
            sources::add(&tx, &r.revision_id, Source::Provider)?;
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
        envelope::write_envelope(dir, id, &file)?;
    }
    envelope::write_envelope(dir, &a.me.device_id(), &env)?;
    log::write_all(dir, &entries)?;
    store.set_author_device(&a.me.device_id())?;
    let floor = (bundle.provider_generation, bundle.provider_manifest_hash.clone());
    kv::put(&store.conn, JOIN_FLOOR_KEY, &floor)?;
    Ok((store, payload.vk, head))
}

/// The join floor, while this device has accepted no provider state yet.
pub fn join_floor(store: &VaultStore) -> Result<Option<(u64, String)>, ErrorCode> {
    kv::get(&store.conn, JOIN_FLOOR_KEY)
}
