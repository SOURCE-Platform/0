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
    let super::served::Served { remote_entries, base_len, rstate, pending, revoked } = super::served::verify(&store, &vk, remote, index, blobs, me)?;
    if revoked {
        return Ok(Applied::Revoked(store));
    }
    // The VK of the committed state. With a pending local rotation, the
    // generation *number* says nothing about the key: another device may
    // have rotated to the same number with a different VK (§11.3 rule 2).
    // - no local rotation and the same generation → our VK;
    // - otherwise (a pending rotation at or past the remote's generation,
    //   or a remote past our base) → the VK from our own envelope in the
    //   served state. Every served state is checked under its own VK.
    let local_gen = store.header.vk_generation;
    let base_gen = pending.as_ref().map_or(local_gen, |p| p.base.vk_generation);
    let rotated = base_gen < local_gen;
    let remote_vk: SecretBytes<32> = if !rotated && remote.vk_generation == local_gen {
        SecretBytes::new(*vk.expose())
    } else if remote.vk_generation >= base_gen {
        let env: DeviceEnvelopeFile = serde_json::from_slice(get(&Role::Env { device_id: me })?).map_err(|_| ErrorCode::WrapCorrupt)?;
        let payload = open_env(&env)?;
        if payload.vk_generation != remote.vk_generation {
            return Err(ErrorCode::ManifestMismatch);
        }
        payload.vk
    } else {
        return Err(ErrorCode::ManifestMismatch);
    };
    remote.checkpoint.verify_binding(&remote_vk, &remote.manifest, &rstate.head, rstate.epoch)?;
    // §11.3 rule 2: a device this Mac is revoking that authorized entries
    // after the base is fork evidence, never adopted (verified above).
    if pending.as_ref().is_some_and(|p| p.all_ops().contains(&pending::PendingOp::Revocation)) {
        for e in remote_entries.iter().skip(base_len) {
            // A recovery epoch that leaves this device active is not a
            // conformant total-loss recovery (S-4 revokes every prior
            // device): with a revocation pending it is fork evidence.
            if e.kind == crate::crypto::registry::EntryKind::RecoveryEpoch {
                return Err(ErrorCode::RegistryFork);
            }
            if let Some(a) = e.authorizer {
                if revoked::is_revoked(&store.conn, &uuid_string(&a))? {
                    return Err(ErrorCode::RegistryFork);
                }
            }
        }
    }
    let header = parse_header(get(&Role::Header)?)?;
    if header.vault_id.0 != vid || header.vk_generation != remote.vk_generation {
        return Err(ErrorCode::ManifestMismatch);
    }
    // §11.3.2 REMOTE_COMMITTED by observation: a verified state carrying
    // the singletons one of our own posted stagings lands (a lost `200`,
    // a restart) settles the record like the `200` would have.
    let landed_base = pending::Base::of(&header);
    let pending = match pending.as_ref().and_then(|p| pending::landed(p, &landed_base)) {
        Some(v) => {
            pending::settle(&store.conn, v, landed_base)?;
            pending::load(&store.conn)?
        }
        None => pending,
    };
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
        // §22.7: anything only D delivered here, and not in the provider
        // state that carries D's revocation, is refused for good.
        let listed: HashSet<[u8; 32]> = authors.keys().copied().collect();
        crate::storage::set_aside::cut_off(&store.conn, &d.device_id, &listed)?;
    }
    let mut store = store;
    for row in &reauthor {
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
        commitment: None,
    };
    let keep_local = match &pending {
        Some(p) if pending::base_unchanged(p, &header) => true,
        Some(p) => {
            let mut p = p.clone();
            p.ops = p.all_ops();
            p.security_driven = p.security();
            p.target_device_ids = p.revocation_targets();
            p.awaiting_redo_targets.clear();
            p.awaiting_redo.clear();
            p.awaiting_security = false;
            p.needs_user = true;
            pending::save(&store.conn, &p)?;
            report.needs_user = true;
            false
        }
        None => false,
    };
    let (mut store, vk) = match remote_vk {
        rvk if !keep_local => {
            let dir = store.dir.clone();
            let reseal = rvk.expose() != vk.expose();
            // §22.7: nothing peer-only is carried into the adopted key.
            let mut store = store;
            if reseal {
                crate::storage::set_aside::set_aside(&mut store, &vk)?;
            }
            // The adopted key's commitment commits with it (§22.4); with no
            // SE identity (tests with software devices) none is staged.
            let commitment = match crate::device::SeDevice::load(&dir) {
                Ok(_) => Some(crate::vault::vk_commit::encode(&dir, &vid, committed.header.vk_generation, &rvk)?),
                Err(_) => None,
            };
            let committed = Adoption { commitment, ..committed };
            adopt(store, &committed, reseal.then_some((&vk, &rvk)))?;
            report.adopted_singletons = true;
            report.adopted_vk = reseal;
            let store = VaultStore::open(&dir)?;
            // A dropped pending revocation revoked nobody remotely: forget
            // local revoked-author marks for devices still active (§11.3
            // adoption path — no false refusals afterwards).
            for d in rstate.devices.iter().filter(|d| !d.revoked) {
                revoked::forget(&store.conn, &d.device_id)?;
            }
            (store, rvk)
        }
        _ => (store, vk),
    };
    // Revisions: those sealed under our current generation merge; others
    // (a rotation we are ahead of) wait for their authors to re-seal
    // them (§11.3, BK-27) — neither admitted nor counted, except a revoked
    // author's, which never will be re-sealed: refused and counted once.
    let gen = store.header.vk_generation;
    let (rows, stale): (Vec<RevisionRow>, Vec<RevisionRow>) = incoming.into_iter().partition(|r| r.vk_generation == gen);
    for r in &stale {
        if revoked::refuses(&store.conn, r)? {
            rev_state::count_refused_once(&store.conn, r, crate::storage::revisions::REFUSED_REVOKED_AUTHOR)?;
        }
    }
    let target = store.flip_target(store.header.clone());
    {
        let tx = store.conn.unchecked_transaction().map_err(|_| ErrorCode::DbCorrupt)?;
        let outcomes = apply_batch(&tx, &rows, gen, &VkCompare { store: &store, vk: &vk })?;
        // §22.7: everything this provider-confirmed state lists is
        // provider-sourced (re-sealable; survives a peer's revocation).
        for e in index.revs() {
            if let crate::backup::index::Role::Rev { revision_id, .. } = &e.role {
                crate::storage::sources::add(&tx, revision_id, crate::storage::sources::Source::Provider)?;
            }
        }
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
