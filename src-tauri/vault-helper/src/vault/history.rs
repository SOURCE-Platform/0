//! §22.4 (F2-D3): retained-history ops and the bulk-deletion gate.
//!
//! A thief with the device's login password passes the presence check, so
//! presence alone deletes single records only; once ten deletions fall
//! inside ten minutes, the next one needs the master password. Anything
//! deleted stays restorable from retained history.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::gate::{finish_authorized, presence_gate};
use super::setup::{emit_panel, PANEL_TIMEOUT_PUB as PANEL_TIMEOUT};
use super::{lock_core, Deps, OpOutcome, PanelOutcome, PanelRequest, VaultCore};
use crate::crypto::hex;
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::state::VaultState;
use crate::storage::store::now_epoch;
use crate::storage::{kv, VaultStore};

const KEY: &str = "deletion_window";
pub const WINDOW_SECS: u64 = 600;
pub const FREE_DELETIONS: usize = 10;

/// Deletion times against a clock that never runs backwards here, so a
/// restart or a changed system clock does not reset the count.
#[derive(Serialize, Deserialize, Default)]
struct Window {
    last: u64,
    times: Vec<u64>,
}

fn load(store: &VaultStore) -> Result<(Window, u64), ErrorCode> {
    let mut w: Window = kv::get(&store.conn, KEY)?.unwrap_or_default();
    let now = now_epoch().max(w.last);
    w.times.retain(|t| now.saturating_sub(*t) < WINDOW_SECS);
    Ok((w, now))
}

/// Whether the next deletion needs the master password.
pub fn needs_mp(store: &VaultStore) -> Result<bool, ErrorCode> {
    Ok(load(store)?.0.times.len() >= FREE_DELETIONS)
}

pub fn record_deletion(store: &VaultStore) -> Result<(), ErrorCode> {
    let (mut w, now) = load(store)?;
    w.times.push(now);
    w.last = now;
    kv::put(&store.conn, KEY, &w)
}

/// After presence: collect and prove the MP when the threshold is reached.
pub fn deletion_gate(core: &Arc<Mutex<VaultCore>>, deps: &Deps) -> Result<(), ErrorCode> {
    let needed = {
        let c = lock_core(core);
        needs_mp(c.store.as_ref().filter(|_| c.state == VaultState::Authorizing).ok_or(ErrorCode::BadState)?)?
    };
    if !needed {
        return Ok(());
    }
    emit_panel(deps, true, PanelRequest::MpEntry);
    let outcome = deps.panel.run(PanelRequest::MpEntry, PANEL_TIMEOUT);
    emit_panel(deps, false, PanelRequest::MpEntry);
    let PanelOutcome::Submitted(mp) = outcome else {
        return Err(ErrorCode::PanelCancelled);
    };
    let c = lock_core(core);
    let store = c.store.as_ref().filter(|_| c.state == VaultState::Authorizing).ok_or(ErrorCode::BadState)?;
    super::recovery_ops::prove_mp(store, &mp).map(|_| ())
}

fn read<T>(core: &Arc<Mutex<VaultCore>>, f: impl FnOnce(&VaultStore, &SecretBytes<32>) -> Result<T, ErrorCode>) -> Result<T, ErrorCode> {
    let c = lock_core(core);
    if c.state != VaultState::Unlocked && c.state != VaultState::Compromised {
        return Err(ErrorCode::BadState);
    }
    match (c.store.as_ref(), c.vk.as_ref()) {
        (Some(store), Some(vk)) => f(store, vk),
        _ => Err(ErrorCode::Internal),
    }
}

/// `list_history {ref}`: metadata of every retained revision.
pub fn list_history(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    let Some(r) = frame.get("ref").and_then(Value::as_str) else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    match read(core, |store, _| store.list_history(r)) {
        Ok(revisions) => OpOutcome::ok(json!({ "revisions": revisions })),
        Err(e) => OpOutcome::err(e),
    }
}

/// `list_deleted`: tombstoned records (ref, kind, last title).
pub fn list_deleted(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    match read(core, |store, vk| store.list_deleted(vk)) {
        Ok(items) => OpOutcome::ok(json!({ "items": items })),
        Err(e) => OpOutcome::err(e),
    }
}

/// `restore_revision {ref, revision_id}`: UNLOCKED + fresh presence.
pub fn restore_revision(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let r = frame.get("ref").and_then(Value::as_str);
    let id = frame.get("revision_id").and_then(Value::as_str).and_then(|h| hex::decode_array::<32>(h));
    let (Some(r), Some(id)) = (r, id) else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    if let Err(o) = presence_gate(core, deps, "Source Vault: restore item") {
        return o;
    }
    finish_authorized!(core, deps, move |store: &mut VaultStore, vk: &SecretBytes<32>| {
        Ok(json!({ "ref": store.restore_revision(vk, r, &id)? }))
    })
}
