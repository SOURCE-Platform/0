//! Applying a verified provider state (spec v0.4 §11.5, §11.3 singleton
//! rule, §3.2 merge and revoked authors, §4.7 envelope catch-up).
//!
//! Order: blobs by hash → registry extends ours (fork/truncation refused)
//! and verifies → manifest signer active → this device's status (revoked
//! → stop, nothing deleted) → the VK of that state (ours, or a newer one
//! from our own envelope) → checkpoint under it → header → newly revoked
//! authors fix `Admit(D)` → singletons adopted unless a pending local
//! change still sits on an unchanged base → revisions merged.

use std::collections::{HashMap, HashSet};

use sha2::{Digest, Sha256};

use super::compare::VkCompare;
use super::pending;
use super::remote::RemoteState;
use super::seen::{self, Seen};
use crate::backup::index::{ObjectIndex, Role};
use crate::backup::object;
use crate::crypto::secret::SecretBytes;
use crate::crypto::wrap::DeviceEnvelopePayload;
use crate::device::envelope::DeviceEnvelopeFile;
use crate::errors::ErrorCode;
use crate::registry::chain::{self, EpochPolicy};
use crate::registry::{file as registry_file, log};
use crate::storage::adopt::{adopt, Adoption};
use crate::storage::header::parse_header;
use crate::storage::merge::{apply_batch, MergeOutcome};
use crate::storage::revisions::{get_row, uuid_string, RevisionRow};
use crate::storage::{rev_state, revoked, VaultStore};

pub type OpenEnvelope<'a> = &'a dyn Fn(&DeviceEnvelopeFile) -> Result<DeviceEnvelopePayload, ErrorCode>;

#[derive(Debug, Default)]
pub struct Report {
    pub generation: u64,
    pub admitted: usize,
    pub refused: usize,
    pub adopted_singletons: bool,
    pub adopted_vk: bool,
    /// The base of a pending local change moved: the user must redo it.
    pub needs_user: bool,
    /// Own revisions re-authored after a refused ancestor (§3.2).
    pub reauthored: usize,
}

pub enum Applied {
    Merged(Report, VaultStore, SecretBytes<32>),
    /// The committed registry revokes this device (§4.7): nothing is
    /// deleted and no key material is touched.
    Revoked(VaultStore),
}

pub fn apply(
    store: VaultStore,
    vk: SecretBytes<32>,
    remote: &RemoteState,
    index: &ObjectIndex,
    blobs: &HashMap<[u8; 32], Vec<u8>>,
    me: [u8; 16],
    open_env: OpenEnvelope<'_>,
) -> Result<Applied, ErrorCode> {
    let vid = store.header.vault_id.0;
    let get = |role: &Role| -> Result<&Vec<u8>, ErrorCode> {
        let e = index.find(role).ok_or(ErrorCode::ManifestMismatch)?;
        let b = blobs.get(&e.blob).ok_or(ErrorCode::BackupObjectMissing)?;
        if <[u8; 32]>::from(Sha256::digest(b)) != e.blob || b.len() as u64 != e.size {
            return Err(ErrorCode::BackupObjectMissing);
        }
        Ok(b)
    };
    // Registry: must extend ours (the pending change's own unpublished
    // entries excepted, §11.3 adoption path) and verify.
    let remote_entries = registry_file::decode(get(&Role::Registry)?)?;
    let local_entries = log::read_entries(&store.dir)?;
    let pending = pending::load(&store.conn)?;
    let base_len = match &pending {
        Some(p) => local_entries
            .iter()
            .position(|e| crate::crypto::registry::entry_hash(e).ok() == Some(p.base.registry_head.0))
            .map_or(local_entries.len(), |i| i + 1),
        None => local_entries.len(),
    };
    chain::check_extends(&local_entries[..base_len.min(local_entries.len())], &remote_entries)?;
    let rstate = chain::verify_chain_with(&remote_entries, &vid, &EpochPolicy::CheckpointAnchored)?;
    if rstate.head != remote.manifest.registry_head {
        return Err(ErrorCode::ManifestMismatch);
    }
    match rstate.devices.iter().find(|d| d.device_id == me) {
        Some(d) if d.revoked => return Ok(Applied::Revoked(store)),
        Some(_) => {}
        None => return Err(ErrorCode::DeviceNotAuthorized), // EV-05: unable to verify
    }
    let signer = rstate.active_device(&remote.manifest.signer_device_id).ok_or(ErrorCode::DeviceNotAuthorized)?;
    remote.manifest.verify(&signer.sign_pub)?;
    // The VK of the committed state.
    // The VK of the committed state: ours, or a newer one from our own
    // envelope. `None` only when our own pending rotation is ahead of it
    // (§11.3): then our chain and the manifest signature anchor it, and
    // its revisions wait for their authors' re-seal.
    let local_gen = store.header.vk_generation;
    let remote_vk: Option<SecretBytes<32>> = if remote.vk_generation == local_gen {
        Some(SecretBytes::new(*vk.expose()))
    } else if remote.vk_generation > local_gen {
        let env: DeviceEnvelopeFile = serde_json::from_slice(get(&Role::Env { device_id: me })?).map_err(|_| ErrorCode::WrapCorrupt)?;
        let payload = open_env(&env)?;
        if payload.vk_generation != remote.vk_generation {
            return Err(ErrorCode::ManifestMismatch);
        }
        Some(payload.vk)
    } else if pending.is_some() {
        None
    } else {
        return Err(ErrorCode::ManifestMismatch);
    };
    if let Some(rvk) = &remote_vk {
        remote.checkpoint.verify_binding(rvk, &remote.manifest, &rstate.head, rstate.epoch)?;
    }
    let header = parse_header(get(&Role::Header)?)?;
    if header.vault_id.0 != vid || header.vk_generation != remote.vk_generation {
        return Err(ErrorCode::ManifestMismatch);
    }
    // Revision rows of the committed state (fetched or already held).
    let mut incoming: Vec<RevisionRow> = Vec::new();
    let mut authors: HashMap<[u8; 32], String> = HashMap::new();
    for e in index.revs() {
        let Role::Rev { record_id, revision_id, .. } = &e.role else { continue };
        let rid = uuid_string(record_id);
        match blobs.get(&e.blob) {
            Some(b) => {
                let row = object::decode_named(b, &e.blob, &rid, revision_id)?;
                authors.insert(*revision_id, row.author_device.clone());
                incoming.push(row);
            }
            None => {
                let held = get_row(&store.conn, revision_id)?.ok_or(ErrorCode::BackupObjectMissing)?;
                authors.insert(*revision_id, held.author_device);
            }
        }
    }
    let mut report = Report { generation: remote.generation, ..Report::default() };
    let me_str = uuid_string(&me);
    // Newly revoked authors: Admit(D) from this (first accepting) index.
    // Our own descendants of a refused revision are re-authored now,
    // under our current VK, so any adoption below re-seals them too.
    let mut reauthor = Vec::new();
    for d in rstate.devices.iter().filter(|d| d.revoked) {
        let author = uuid_string(&d.device_id);
        let admit: HashSet<[u8; 32]> = authors.iter().filter(|(_, a)| **a == author).map(|(id, _)| *id).collect();
        reauthor.extend(revoked::record(&store.conn, &d.device_id, &admit, &me_str)?);
    }
    let mut store = store;
    for row in reauthor.iter().filter(|r| !r.deleted) {
        if store.reauthor(&vk, row).is_ok() {
            report.reauthored += 1;
        }
    }
    // Singletons (§11.3 rule).
    let committed = Adoption {
        header: header.clone(),
        wrap_mp: get(&Role::WrapMp)?.clone(),
        wrap_rk: index.find(&Role::WrapRk).map(|_| get(&Role::WrapRk).cloned()).transpose()?,
        envelopes: index.envs().map(|(id, e)| Ok((*id, blobs.get(&e.blob).ok_or(ErrorCode::BackupObjectMissing)?.clone()))).collect::<Result<_, ErrorCode>>()?,
        registry: get(&Role::Registry)?.clone(),
    };
    let keep_local = match &pending {
        Some(p) if pending::base_unchanged(p, &header) => true,
        Some(p) => {
            let mut p = p.clone();
            p.needs_user = true;
            pending::save(&store.conn, &p)?;
            report.needs_user = true;
            false
        }
        None => false,
    };
    let (mut store, vk) = match remote_vk {
        Some(rvk) if !keep_local => {
            let dir = store.dir.clone();
            let reseal = remote.vk_generation != local_gen;
            adopt(store, &committed, reseal.then_some((&vk, &rvk)))?;
            report.adopted_singletons = true;
            report.adopted_vk = reseal;
            (VaultStore::open(&dir)?, rvk)
        }
        _ => (store, vk),
    };
    // Revisions: those sealed under our current generation merge; others
    // (a rotation we are ahead of) wait for their authors to re-seal
    // them (§11.3, BK-27) — neither admitted nor counted.
    let gen = store.header.vk_generation;
    let rows: Vec<RevisionRow> = incoming.into_iter().filter(|r| r.vk_generation == gen).collect();
    let target = store.flip_target(store.header.clone());
    {
        let tx = store.conn.unchecked_transaction().map_err(|_| ErrorCode::DbCorrupt)?;
        let outcomes = apply_batch(&tx, &rows, gen, &VkCompare { store: &store, vk: &vk })?;
        report.admitted = outcomes.iter().filter(|o| matches!(o, MergeOutcome::Applied { .. })).count();
        report.refused = outcomes.iter().filter(|o| matches!(o, MergeOutcome::Rejected(_))).count();
        // Anything still held has a refused ancestor: never applicable.
        if rev_state::pending_count(&tx)? != 0 {
            rev_state::purge_pending(&tx)?;
        }
        crate::storage::flip::stamp(&tx, &target)?;
        tx.commit().map_err(|_| ErrorCode::DbCorrupt)?;
    }
    store.persist_head()?;
    seen::save(&store.conn, &Seen::with_auth(remote.generation, remote.manifest_hash, remote.state_commit, &remote.recovery_auth))?;
    Ok(Applied::Merged(report, store, vk))
}
