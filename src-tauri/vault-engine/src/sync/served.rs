//! Verifying a served state before any key of it is opened (spec v0.4
//! §11.5, §4.7; factored out of `apply` for F.2d step 1, review SEC-B1):
//! the served registry extends ours and verifies, its head is the
//! manifest's, any new recovery epoch carries its proof, the manifest
//! signer is active in it and the signature holds, and this device's own
//! status. The apply runs it first; the master-password adoption prompt
//! runs it before asking, so only a state that passes all of this can
//! raise the panel. Pure and fast: called under the core mutex.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use super::pending::{self, PendingRemote};
use super::remote::RemoteState;
use crate::backup::index::{ObjectIndex, Role};
use crate::crypto::registry::RegistryEntry;
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::registry::chain::{self, EpochPolicy, RegistryState};
use crate::registry::{file as registry_file, log};
use crate::storage::VaultStore;

/// A served blob, exactly as the index names it (hash and size).
pub fn blob<'a>(index: &ObjectIndex, blobs: &'a HashMap<[u8; 32], Vec<u8>>, role: &Role) -> Result<&'a Vec<u8>, ErrorCode> {
    let e = index.find(role).ok_or(ErrorCode::ManifestMismatch)?;
    let b = blobs.get(&e.blob).ok_or(ErrorCode::BackupObjectMissing)?;
    if <[u8; 32]>::from(Sha256::digest(b)) != e.blob || b.len() as u64 != e.size {
        return Err(ErrorCode::BackupObjectMissing);
    }
    Ok(b)
}

pub struct Served {
    pub remote_entries: Vec<RegistryEntry>,
    pub base_len: usize,
    pub rstate: RegistryState,
    pub pending: Option<PendingRemote>,
    /// The committed registry revokes this device (§4.7).
    pub revoked: bool,
}

pub fn verify(store: &VaultStore, vk: &SecretBytes<32>, remote: &RemoteState, index: &ObjectIndex, blobs: &HashMap<[u8; 32], Vec<u8>>, me: [u8; 16]) -> Result<Served, ErrorCode> {
    let vid = store.header.vault_id.0;
    // Registry: must extend ours (the pending change's own unpublished
    // entries excepted, §11.3 adoption path) and verify.
    let remote_entries = registry_file::decode(blob(index, blobs, &Role::Registry)?)?;
    let local_entries = log::read_entries(&store.dir)?;
    let pending = pending::load(&store.conn)?;
    let base_len = match &pending {
        Some(p) => local_entries
            .iter()
            .position(|e| crate::crypto::registry::entry_hash(e).ok() == Some(p.base.registry_head.0))
            .map_or(local_entries.len(), |i| i + 1),
        None => local_entries.len(),
    };
    // The served chain and manifest verify on their own terms first; a
    // divergence from ours is fork evidence only when a device of *our*
    // registry signed the served manifest (SPEC-B2): anything else is
    // unverifiable and refused with no state change.
    let not_fork = |e: ErrorCode| if e == ErrorCode::RegistryFork { ErrorCode::SignatureInvalid } else { e };
    let rstate = chain::verify_chain_with(&remote_entries, &vid, &EpochPolicy::CheckpointAnchored).map_err(not_fork)?;
    if rstate.head != remote.manifest.registry_head {
        return Err(ErrorCode::ManifestMismatch);
    }
    // A recovery epoch this device has not yet accepted is authorized only
    // by its proof, under the VK of the manifest it binds — which must be
    // the state this device last accepted, at the VK it holds. The served
    // checkpoint cannot anchor such an epoch: its VK comes from an envelope
    // anyone can seal to our public key (HPKE base mode), so it would only
    // vouch for itself (review SEC-B3). Unprovable → refused, no change.
    let rotated_locally = pending.as_ref().is_some_and(|p| p.base.vk_generation < store.header.vk_generation);
    let accepted = super::seen::load(&store.conn)?;
    for e in remote_entries.iter().skip(base_len).filter(|e| e.kind == crate::crypto::registry::EntryKind::RecoveryEpoch) {
        let binds = e.manifest_hash.ok_or(ErrorCode::SignatureInvalid)?;
        let proven = !rotated_locally
            && accepted.as_ref().is_some_and(|s| s.manifest_hash.0 == binds)
            && crate::crypto::registry::verify_recovery_proof(vk, &binds, e).is_ok();
        if !proven {
            return Err(ErrorCode::SignatureInvalid);
        }
    }
    let signer = rstate.active_device(&remote.manifest.signer_device_id).ok_or(ErrorCode::DeviceNotAuthorized)?;
    remote.manifest.verify(&signer.sign_pub)?;
    if let Err(e) = chain::check_extends(&local_entries[..base_len.min(local_entries.len())], &remote_entries) {
        let verified = e == ErrorCode::RegistryFork && super::fetch::signed_by_our_registry(store, remote)?;
        return Err(if verified { e } else { not_fork(e) });
    }
    let revoked = match rstate.devices.iter().find(|d| d.device_id == me) {
        Some(d) => d.revoked,
        None => return Err(ErrorCode::DeviceNotAuthorized), // EV-05: unable to verify
    };
    Ok(Served { remote_entries, base_len, rstate, pending, revoked })
}
