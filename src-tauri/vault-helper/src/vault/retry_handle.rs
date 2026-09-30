//! §1.5 `setup_retry_handle` (BK-28): the first `create` met
//! `HANDLE_TAKEN`. Its bootstrap blobs — including a `recovery.wrap` of
//! the current VK under the first RK — may already sit at the provider,
//! and RK bytes are not retained (§2.11), so a new handle comes with a new
//! Recovery Key and one journaled VK rotation: records, `password.wrap`
//! (same PK), `recovery.wrap` (new RK, new `auth_salt_rk`), the genesis
//! envelope, the header, the `kv` handle and the `pending_remote` update
//! commit together. The first sheet then opens only the retired VK, which
//! protects nothing that was ever published.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde_json::{json, Value};

use super::rk_ops::{authorize_pub, finish_pub, make_sheet, show_sheet, HANDLE_KEY};
use super::setup::{emit_panel, PANEL_TIMEOUT_PUB};
use super::{ev_state, lock_core, Deps, OpOutcome, PanelOutcome, PanelRequest, SheetReason, VaultCore};
use crate::crypto::secret::{random_secret, SecretBytes};
use crate::device::SeDevice;
use crate::errors::ErrorCode;
use crate::registry::chain::EpochPolicy;
use crate::state::VaultState;
use crate::storage::header::Header;
use crate::storage::rotation::{self, ExtraStaging, MpWrap, RkWrap};
use crate::storage::VaultStore;
use crate::sync::change::RemoteChange;
use crate::sync::{pending, seen};

/// The rotation's extra staging: the `vault_create` pending update (new
/// RK class) plus the new handle, in the rotation's own commit.
struct Retry<'a> {
    change: RemoteChange<'a>,
    handle: &'a str,
}

impl ExtraStaging for Retry<'_> {
    fn stage(&self, dir: &std::path::Path, new_vk: &SecretBytes<32>, new_vk_generation: u32) -> Result<Vec<String>, ErrorCode> {
        self.change.stage(dir, new_vk, new_vk_generation)
    }

    fn stage_db(&self, conn: &Connection, new_header: &Header) -> Result<(), ErrorCode> {
        self.change.stage_db(conn, new_header)?;
        crate::storage::kv::put(conn, HANDLE_KEY, &self.handle)
    }
}

/// Only an UNLOCKED vault whose `create` never committed can retry.
fn create_pending(core: &Arc<Mutex<VaultCore>>) -> Result<(), ErrorCode> {
    let c = lock_core(core);
    let store = c.store.as_ref().filter(|_| c.state == VaultState::Unlocked).ok_or(ErrorCode::BadState)?;
    let p = pending::load(&store.conn)?;
    if seen::load(&store.conn)?.is_some() || !p.is_some_and(|p| p.ops.contains(&pending::PendingOp::VaultCreate)) {
        return Err(ErrorCode::BadState);
    }
    Ok(())
}

/// `setup_retry_handle {handle}` → `{publication}`, the re-staged create.
pub fn setup_retry_handle(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let Some(handle) = frame.get("handle").and_then(Value::as_str).and_then(|h| vault_proto::handle::normalize(h).ok()) else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    if let Err(e) = create_pending(core) {
        return OpOutcome::err(e);
    }
    if let Err(e) = authorize_pub(core, deps, "Source Vault: choose a new recovery name") {
        return finish_pub(core, deps, Err(e));
    }
    emit_panel(deps, true, PanelRequest::MpEntry);
    let outcome = deps.panel.run(PanelRequest::MpEntry, PANEL_TIMEOUT_PUB);
    emit_panel(deps, false, PanelRequest::MpEntry);
    let PanelOutcome::Submitted(mp) = outcome else {
        return finish_pub(core, deps, Err(ErrorCode::PanelCancelled));
    };
    let proved = {
        let c = lock_core(core);
        let Some(store) = c.store.as_ref().filter(|_| c.state == VaultState::Authorizing) else {
            return OpOutcome::err(ErrorCode::BadState);
        };
        super::recovery_ops::prove_mp_resident(&c, &mp).map(|pk| (pk, store.header.vault_id.0, store.header.registry_head.0))
    };
    drop(mp);
    let (pk, vault_id, head) = match proved {
        Ok(v) => v,
        Err(e) => return finish_pub(core, deps, Err(e)),
    };
    // The new key is seen and acknowledged before anything commits.
    let rk = random_secret();
    if !show_sheet(deps, &make_sheet(&rk, &vault_id, 1, &head, Some(&handle), SheetReason::HandleRetried)) {
        return finish_pub(core, deps, Err(ErrorCode::PanelCancelled));
    }
    let mut c = lock_core(core);
    if c.state != VaultState::Authorizing {
        return OpOutcome::err(ErrorCode::BadState); // a lock landed meanwhile
    }
    let (Some(store), Some(vk)) = (c.store.take(), c.vk.take()) else {
        return OpOutcome::err(ErrorCode::BadState);
    };
    c.provider.publish = None; // the first create is void
    let dir = store.dir.clone();
    let rotated = rotate(store, &vk, &pk, &rk, &handle);
    drop(vk);
    let restaged = rotated.and_then(|new_vk| {
        let store = VaultStore::open(&dir)?;
        let me = SeDevice::load(&dir)?;
        let reg = crate::registry::log::read_state(&dir, &store.header.vault_id.0, &EpochPolicy::CheckpointAnchored)?;
        let st = super::backup_ops::restage_create(&store, &reg, &new_vk, &me)?;
        Ok((store, new_vk, st))
    });
    match restaged {
        Ok((store, new_vk, staging)) => {
            let t = super::backup_ops::transfer_for(&staging);
            let summary = super::backup_ops::staging_summary(&t, &staging);
            let _ = crate::sync::staged_disk::persist(&store, &staging); // §22.11 (best effort)
            c.header = Some(store.header.clone());
            c.store = Some(store);
            c.vk = Some(new_vk.mlock_best_effort());
            c.provider.publish = Some(super::provider_ops::PublishSession { t, staging });
            c.state = VaultState::Unlocked;
            c.note_authorization();
            deps.events.emit(ev_state(c.reported_state()));
            OpOutcome::ok(json!({ "publication": summary }))
        }
        Err(e) => {
            // The journal leaves the old or the new state on disk; LOCKED
            // opens whichever committed (§2.10).
            for ev in c.lock(super::LockReason::Fatal) {
                deps.events.emit(ev);
            }
            OpOutcome::err(e)
        }
    }
}

/// The journaled rotation: every active device (the genesis Mac)
/// re-enveloped, both wraps rebuilt, handle and pending in one commit.
pub fn rotate(store: VaultStore, vk: &SecretBytes<32>, pk: &SecretBytes<32>, rk: &SecretBytes<32>, handle: &str) -> Result<SecretBytes<32>, ErrorCode> {
    rotate_failing(store, vk, pk, rk, handle, None)
}

/// `rotate` with the journal's test-only crash injection (BK-28: a crash
/// at any point leaves the old or the new state, never a mix).
pub fn rotate_failing(
    store: VaultStore,
    vk: &SecretBytes<32>,
    pk: &SecretBytes<32>,
    rk: &SecretBytes<32>,
    handle: &str,
    fail: Option<crate::storage::rotation_journal::FailAt>,
) -> Result<SecretBytes<32>, ErrorCode> {
    let vid = store.header.vault_id.0;
    let reg = crate::registry::log::read_state(&store.dir, &vid, &EpochPolicy::CheckpointAnchored)?;
    let envelopes = crate::device::rotate::EnvelopePlan {
        vault_id: vid,
        devices: reg.devices.iter().filter(|d| !d.revoked).map(|d| (d.device_id, d.agree_pub)).collect(),
        fresh: Vec::new(),
    };
    let base = pending::load(&store.conn)?.ok_or(ErrorCode::BadState)?.base;
    let change = RemoteChange { envelopes: Some(&envelopes), op: pending::PendingOp::VaultCreate, security_driven: false, base, mp: None, rk: Some(rk), revoke: None, registry: None };
    let retry = Retry { change, handle };
    Ok(rotation::rotate(store, vk, MpWrap::Reseal(pk), RkWrap::SealNew(rk), Some(&retry), fail)?.new_vk)
}
