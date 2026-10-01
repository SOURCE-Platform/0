//! Sync ops (spec v0.4 §1.5, §11.5): `backup_state_offer {state}` checks
//! the served state against this device's rollback floor and asks for its
//! index; `backup_apply {session}` first turns the index into the list of
//! blobs still needed, then — once every blob arrived through the §1.3
//! streams — verifies and merges in one step. UNLOCKED → SYNCING →
//! UNLOCKED; in RECOVERING both ops drive the recovery session instead.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::provider_ops::{session_id, SyncSession};
use super::{ev_state, lock_core, Deps, OpOutcome, VaultCore};
use crate::backup::index::Role;
use crate::crypto::hex;
use crate::crypto::secret::SecretBytes;
use crate::device::{envelope, SeDevice};
use crate::errors::ErrorCode;
use crate::state::VaultState;
use crate::sync::apply::{self, Applied};
use crate::sync::fetch::{self, Offer};
use crate::sync::remote;
use crate::sync::session::Transfer;

pub const CAP_INDEX: u64 = 8 << 20;
pub const CAP_REGISTRY: u64 = 4 << 20;
pub const CAP_BLOB: u64 = 1 << 20;

pub fn cap(role: &Role) -> u64 {
    match role {
        Role::Registry => CAP_REGISTRY,
        _ => CAP_BLOB,
    }
}

fn need_json(t: &Transfer) -> Value {
    json!(t.still_needed().iter().map(hex::encode).collect::<Vec<_>>())
}

/// `backup_state_offer {state}` (the provider's `state_get` JSON).
pub fn backup_state_offer(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let run = || -> Result<Value, ErrorCode> {
        let raw = frame.get("state").and_then(Value::as_str).ok_or(ErrorCode::InvalidInput)?;
        let remote = remote::parse(raw.as_bytes())?;
        let mut c = lock_core(core);
        if c.state == VaultState::Recovering {
            let r = c.provider.recovery.as_mut().ok_or(ErrorCode::BadState)?;
            if remote.manifest.vault_id != r.rec.locate.vault_id {
                return Err(ErrorCode::RecoveryMetadataMismatch);
            }
            r.t.expect([(remote.manifest.object_index_hash, CAP_INDEX)]);
            r.remote = Some(remote);
            return Ok(json!({ "session": hex::encode(r.t.id), "need": need_json(&r.t) }));
        }
        if c.state != VaultState::Unlocked || c.provider.sync.is_some() {
            return Err(ErrorCode::BadState);
        }
        let store = c.store.as_ref().ok_or(ErrorCode::Internal)?;
        match fetch::offer(store, &remote) {
            Ok(Offer::UpToDate) => Ok(json!({ "up_to_date": true })),
            Ok(Offer::Index(h)) => {
                let mut t = Transfer::new(Default::default());
                t.expect([(h, CAP_INDEX)]);
                let out = json!({ "session": hex::encode(t.id), "need": need_json(&t) });
                c.provider.sync = Some(SyncSession { t, remote, index: None });
                deps.events.emit(ev_state(c.reported_state()));
                Ok(out)
            }
            // §22.14: while behind, "our registry" is a restored older copy,
            // so nothing it vouches for is fork evidence.
            Err(ErrorCode::RegistryFork) if c.behind => Err(ErrorCode::SignatureInvalid),
            Err(ErrorCode::RegistryFork) => {
                // §4.6: verified fork evidence (fetch::offer checked the
                // served manifest's signature against our registry).
                enter_compromised(&mut c, deps);
                Err(ErrorCode::RegistryFork)
            }
            Err(e) => Err(e),
        }
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}

/// `backup_apply {session}`: `{need}` while blobs are missing, then the
/// merge report.
pub fn backup_apply(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let id = match session_id(frame) {
        Ok(i) => i,
        Err(e) => return OpOutcome::err(e),
    };
    let recovering = lock_core(core).state == VaultState::Recovering;
    if recovering {
        return super::recovery_flow::recovery_apply(core, &id);
    }
    let run = || -> Result<Value, ErrorCode> {
        let mut c = lock_core(core);
        let c = &mut *c;
        let session = c.provider.sync.as_mut().filter(|s| s.t.id == id).ok_or(ErrorCode::TransferInvalid)?;
        if session.index.is_none() {
            let h = session.remote.manifest.object_index_hash;
            let bytes = session.t.received.get(&h).ok_or(ErrorCode::TransferInvalid)?.clone();
            let (index, need) = fetch::plan(c.store.as_ref().ok_or(ErrorCode::Internal)?, &session.remote, &bytes)?;
            let caps: Vec<([u8; 32], u64)> = need
                .iter()
                .map(|h| (*h, index.entries.iter().find(|e| &e.blob == h).map_or(CAP_BLOB, |e| cap(&e.role))))
                .collect();
            session.t.expect(caps);
            session.index = Some(index);
            if !session.t.still_needed().is_empty() {
                return Ok(json!({ "need": need_json(&session.t) }));
            }
        }
        if !session.t.still_needed().is_empty() {
            return Ok(json!({ "need": need_json(&session.t) }));
        }
        let session = c.provider.sync.take().ok_or(ErrorCode::Internal)?;
        if c.behind {
            // §22.14: the restored store's registry is not the anchor; the
            // served one must contain the head this helper accepted.
            let index = session.index.as_ref().ok_or(ErrorCode::Internal)?;
            let reg = index.find(&Role::Registry).and_then(|e| session.t.received.get(&e.blob)).ok_or(ErrorCode::ManifestMismatch)?;
            let entries = crate::registry::file::decode(reg).map_err(|_| ErrorCode::SignatureInvalid)?;
            if !super::floor::anchored(c.store.as_ref().ok_or(ErrorCode::Internal)?, &entries)? {
                deps.events.emit(ev_state(c.reported_state()));
                return Err(ErrorCode::SignatureInvalid);
            }
        }
        let (store, vk) = (c.store.take().ok_or(ErrorCode::Internal)?, c.vk.take().ok_or(ErrorCode::Internal)?);
        let dir = store.dir.clone();
        // A refused state never costs the user their unlocked vault
        // (§11.6, §15 "reject; keep local"): the VK is kept for the
        // failure path, valid only while the committed singletons are
        // exactly those it was used with (an adoption may re-key at the
        // same generation number, §11.3 rule 2).
        let keep = (SecretBytes::new(*vk.expose()), crate::sync::pending::Base::of(&store.header));
        let me = SeDevice::load(&c.vault_dir)?;
        let (tag, vid) = (me.key_tag().to_string(), store.header.vault_id.0);
        let open = move |f: &envelope::DeviceEnvelopeFile| envelope::open_envelope(&tag, &vid, f);
        let index = session.index.as_ref().ok_or(ErrorCode::Internal)?;
        let result = apply::apply(store, vk, &session.remote, index, &session.t.received, crate::registry::device::DeviceIdentity::device_id(&me), &open);
        let out = match result {
            Ok(Applied::Merged(rep, store, vk)) => {
                c.header = Some(store.header.clone());
                c.store = Some(store);
                c.vk = Some(vk.mlock_best_effort());
                // A verified adoption may have brought a new key.
                super::vk_commit::commit_resident(c);
                Ok(json!({
                    "generation": rep.generation, "admitted": rep.admitted, "refused": rep.refused,
                    "adopted_vk": rep.adopted_vk, "needs_user": rep.needs_user, "reauthored": rep.reauthored,
                }))
            }
            Ok(Applied::Revoked(store)) => {
                // §4.7: the committed registry revokes this device. Nothing
                // is deleted; the vault locks (its key material stays
                // where it is) and the UI reports the removal.
                drop(store);
                c.state = VaultState::Locked;
                Ok(json!({ "revoked": true }))
            }
            // §22.14 (review SEC-I1): while behind, the restored copy's own
            // registry vouches for nothing, so no fork is acted on.
            Err(ErrorCode::RegistryFork) if c.behind => {
                c.store = crate::storage::VaultStore::open(&dir).ok();
                match c.store.as_ref().map(|s| s.header.clone()) {
                    Some(h) if crate::sync::pending::Base::of(&h) == keep.1 => {
                        c.header = Some(h);
                        c.vk = Some(keep.0.mlock_best_effort());
                        c.state = VaultState::Unlocked;
                    }
                    _ => {
                        c.store = None;
                        c.state = VaultState::Locked;
                    }
                }
                Err(ErrorCode::SignatureInvalid)
            }
            Err(e) => {
                // The apply is journaled/transactional: reopen whatever is
                // committed. The VK comes back only if the committed state
                // is still at its generation (an adoption may have landed).
                c.store = crate::storage::VaultStore::open(&dir).ok();
                match c.store.as_ref().map(|s| s.header.clone()) {
                    None => c.state = VaultState::Error,
                    Some(h) if crate::sync::pending::Base::of(&h) == keep.1 => {
                        c.header = Some(h);
                        c.vk = Some(keep.0.mlock_best_effort());
                        c.state = VaultState::Unlocked;
                        if e == ErrorCode::RegistryFork {
                            enter_compromised(c, deps);
                        }
                    }
                    Some(h) => {
                        c.header = Some(h);
                        let recorded = e != ErrorCode::RegistryFork || c.store.as_ref().is_some_and(|s| crate::storage::compromised::mark(&s.conn, e).is_ok());
                        if let Some(s) = c.store.as_ref() {
                            let _ = crate::sync::staged_disk::forget(&s.dir, &s.conn); // §22.11 terminal
                        }
                        c.provider.publish = None;
                        c.store = None;
                        // Re-entered COMPROMISED at unlock; evidence that
                        // cannot be recorded fails closed to ERROR.
                        c.state = if recorded { VaultState::Locked } else { VaultState::Error };
                    }
                }
                Err(e)
            }
        };
        deps.events.emit(ev_state(c.reported_state()));
        out
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}

/// §4.6/§13.2: record the evidence and freeze writes. A staged
/// publication from before the fork is dropped (it would write). If the
/// evidence cannot be recorded the vault fails closed to ERROR.
fn enter_compromised(c: &mut VaultCore, deps: &Deps) {
    c.provider.publish = None;
    c.provider.sync = None;
    if let Some(s) = c.store.as_ref() {
        let _ = crate::sync::staged_disk::forget(&s.dir, &s.conn); // §22.11 terminal
    }
    let marked = c.store.as_ref().map(|s| crate::storage::compromised::mark(&s.conn, ErrorCode::RegistryFork));
    if matches!(marked, Some(Ok(()))) {
        c.state = VaultState::Compromised;
    } else {
        c.enter_error(&deps.events);
        return;
    }
    deps.events.emit(ev_state(c.state));
}

