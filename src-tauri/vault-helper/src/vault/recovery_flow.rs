//! Total-loss recovery ops (spec v0.4 §1.5, §11.5, §11.8):
//! `recovery_begin {kind, locate_response}` checks the provider origin
//! and the KDF policy **before** the panel collects the MP or RK, then
//! derives the recovery-auth key → RECOVERING. The served state arrives
//! through `backup_state_offer` / streams / `backup_apply` (verification
//! and the FR-01 preview), `recovery_complete` re-encrypts and stages the
//! finalize (a new RK is shown and must be acknowledged first), and the
//! finalize's `backup_commit_result` lands the device in UNLOCKED.
//!
//! This build recovers into an empty vault directory (UNINITIALIZED);
//! recovery over a LOCKED local vault is refused (documented follow-up).

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::backup_ops::{staging_summary, transfer_for};
use super::provider_ops::RecoverySession;
use super::rk_ops::{make_sheet, show_sheet};
use super::setup::emit_panel;
use super::{ev_state, lock_core, Deps, OpOutcome, PanelOutcome, PanelRequest, SheetReason, VaultCore};
use crate::crypto::{bip39, hex};
use crate::device::SeDevice;
use crate::errors::ErrorCode;
use crate::recovery::complete::Plan;
use crate::recovery::locate;
use crate::recovery::total_loss::{Credential, Recovery};
use crate::registry::device::PLATFORM_MACOS;
use crate::state::VaultState;
use crate::storage::header::{check_provider, default_provider};
use crate::storage::store::now_epoch;
use crate::storage::VaultStore;
use crate::sync::publish;
use crate::sync::session::Transfer;

const PANEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
pub(crate) const STAGING: &str = "recovered";

pub fn recovery_begin(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let run = || -> Result<Value, ErrorCode> {
        let kind = frame.get("kind").and_then(Value::as_str).ok_or(ErrorCode::InvalidInput)?;
        let handle = frame.get("handle").and_then(Value::as_str).map(vault_proto::handle::normalize).transpose().map_err(|_| ErrorCode::InvalidInput)?;
        let locate_json = frame.get("locate_response").and_then(Value::as_str).ok_or(ErrorCode::InvalidInput)?.to_string();
        {
            let c = lock_core(core);
            if c.state != VaultState::Uninitialized || c.provider.recovery.is_some() {
                return Err(ErrorCode::BadState);
            }
        }
        // §11.5: origin allowlist and KDF policy before any prompt.
        let origin = default_provider()?;
        check_provider(origin)?;
        locate::parse(locate_json.as_bytes())?;
        let req = match kind {
            "mp" => PanelRequest::MpEntry,
            "rk" => PanelRequest::RkEntry,
            _ => return Err(ErrorCode::InvalidInput),
        };
        emit_panel(deps, true, req);
        let outcome = deps.panel.run(req, PANEL_TIMEOUT);
        emit_panel(deps, false, req);
        let PanelOutcome::Submitted(secret) = outcome else { return Err(ErrorCode::PanelCancelled) };
        let rec = if kind == "mp" {
            Recovery::begin(origin, locate_json.as_bytes(), Credential::Mp(&secret), now_epoch())?
        } else {
            let rk = std::str::from_utf8(&secret).ok().map(bip39::decode_rk).ok_or(ErrorCode::RecoveryKeyInvalid)?.map_err(|_| ErrorCode::RecoveryKeyInvalid)?;
            Recovery::begin(origin, locate_json.as_bytes(), Credential::Rk(&rk), now_epoch())?
        };
        drop(secret);
        let mut c = lock_core(core);
        if c.state != VaultState::Uninitialized {
            return Err(ErrorCode::BadState);
        }
        let t = Transfer::new(Default::default());
        let out = json!({ "session": hex::encode(t.id), "origin": origin, "vault_id": hex::encode(rec.locate.vault_id) });
        c.provider.recovery = Some(RecoverySession { t, rec, remote: None, index: None, preview: None, completed: None, acknowledged: false, handle });
        c.state = VaultState::Recovering;
        deps.events.emit(ev_state(VaultState::Recovering));
        Ok(out)
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}

/// `backup_apply` in RECOVERING: index → need every blob → verify.
pub fn recovery_apply(core: &Arc<Mutex<VaultCore>>, id: &[u8; 16]) -> OpOutcome {
    let run = || -> Result<Value, ErrorCode> {
        let mut c = lock_core(core);
        let r = c.provider.recovery.as_mut().filter(|r| &r.t.id == id).ok_or(ErrorCode::TransferInvalid)?;
        let remote = r.remote.clone().ok_or(ErrorCode::BadState)?;
        if r.index.is_none() {
            let bytes = r.t.received.get(&remote.manifest.object_index_hash).ok_or(ErrorCode::TransferInvalid)?.clone();
            let index = r.rec.plan(&remote, &bytes)?;
            let caps: Vec<([u8; 32], u64)> = index.entries.iter().map(|e| (e.blob, super::sync_ops::cap(&e.role))).collect();
            r.t.expect(caps);
            r.index = Some(index);
        }
        let need = r.t.still_needed();
        if !need.is_empty() {
            return Ok(json!({ "need": need.iter().map(hex::encode).collect::<Vec<_>>() }));
        }
        let index = r.index.clone().ok_or(ErrorCode::Internal)?;
        let p = r.rec.verify(remote, &index, &r.t.received)?;
        let out = preview_json(&p);
        r.preview = Some(p);
        Ok(out)
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}

fn preview_json(p: &crate::recovery::total_loss::Preview) -> Value {
    json!({ "preview": {
        "vault_id": hex::encode(p.vault_id), "generation": p.generation, "created_at": p.created_at,
        "item_count": p.item_count, "registry_head_prefix": p.registry_head_prefix,
    }})
}

/// `recovery_preview {session}`: FR-01 data, shown before completion.
pub fn recovery_preview(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    let c = lock_core(core);
    match c.provider.recovery.as_ref().and_then(|r| r.preview.as_ref()) {
        Some(p) => OpOutcome::ok(preview_json(p)),
        None => OpOutcome::err(ErrorCode::BadState),
    }
}

/// `recovery_complete {session}`: RK path → the panel sets a new MP; MP
/// path → a new RK is issued and must be acknowledged on its sheet before
/// anything is uploaded.
pub fn recovery_complete(core: &Arc<Mutex<VaultCore>>, deps: &Deps) -> OpOutcome {
    let run = || -> Result<Value, ErrorCode> {
        let (mut session, dir) = {
            let mut c = lock_core(core);
            // A completed, acknowledged attempt whose upload failed
            // transiently is resumed as staged (no second rotation).
            if let Some(r) = c.provider.recovery.as_ref().filter(|r| r.acknowledged) {
                if let Some(done) = &r.completed {
                    return Ok(staging_summary(&r.t, &done.staging));
                }
            }
            if !c.provider.recovery.as_ref().is_some_and(|r| r.preview.is_some() && r.completed.is_none()) {
                return Err(ErrorCode::BadState);
            }
            let r = c.provider.recovery.take().ok_or(ErrorCode::BadState)?;
            (r, c.vault_dir.clone())
        };
        let new_mp = if session.rec.class == crate::crypto::recovery_auth::RecoveryClass::Rk {
            emit_panel(deps, true, PanelRequest::MpCreate);
            let o = deps.panel.run(PanelRequest::MpCreate, PANEL_TIMEOUT);
            emit_panel(deps, false, PanelRequest::MpCreate);
            match o {
                PanelOutcome::Submitted(mp) => Some(mp),
                _ => return abort(core, session, ErrorCode::PanelCancelled),
            }
        } else {
            None
        };
        let me = SeDevice::create(&dir, "This Mac", PLATFORM_MACOS)?;
        let staging_dir = dir.join(STAGING);
        let plan = Plan { new_mp: new_mp.as_ref().map(|m| m.as_slice()), keep_rk: None };
        let done = match session.rec.complete(&staging_dir, &me, plan) {
            Ok(d) => d,
            Err(e) => return abort(core, session, e),
        };
        if let Some(h) = &session.handle {
            if let Err(e) = crate::storage::kv::put(&done.store.conn, super::rk_ops::HANDLE_KEY, h) {
                drop(done);
                let _ = std::fs::remove_dir_all(&staging_dir);
                return abort(core, session, e);
            }
        }
        if let Some(rk) = &done.new_rk {
            let st = &done.staging;
            let sheet = make_sheet(rk, &session.rec.locate.vault_id, st.generation, &done.store.header.registry_head.0, session.handle.as_deref(), SheetReason::Recovered);
            if !show_sheet(deps, &sheet) {
                drop(done);
                let _ = std::fs::remove_dir_all(&staging_dir);
                return abort(core, session, ErrorCode::PanelCancelled);
            }
        }
        session.t = transfer_for(&done.staging);
        let out = staging_summary(&session.t, &done.staging);
        session.completed = Some(done);
        session.acknowledged = true;
        let mut c = lock_core(core);
        if c.state != VaultState::Recovering {
            return Err(ErrorCode::BadState); // a lock ended the recovery meanwhile
        }
        c.provider.recovery = Some(session);
        Ok(out)
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}

fn abort(core: &Arc<Mutex<VaultCore>>, session: RecoverySession, e: ErrorCode) -> Result<Value, ErrorCode> {
    // The session stays usable for another attempt (e.g. a cancelled
    // panel) unless a lock ended the recovery; nothing was uploaded.
    let mut c = lock_core(core);
    if c.state == VaultState::Recovering {
        c.provider.recovery = Some(session);
    }
    Err(e)
}

/// The finalize's provider outcome. `200` → the recovered vault moves
/// from staging into place and the device is UNLOCKED on the fresh VK. A
/// transient failure keeps the completed attempt for a byte-identical
/// re-send (the finalize may even have landed); a refusal discards it.
pub fn finalize_result(core: &Arc<Mutex<VaultCore>>, status: u64, body: &Value, deps: &Deps) -> OpOutcome {
    let run = || -> Result<Value, ErrorCode> {
        let mut c = lock_core(core);
        let code = body["error"].as_str().unwrap_or("BACKUP_UNAVAILABLE").to_string();
        if status != 200 && super::backup_ops::transient(status) {
            c.provider.recovery.as_ref().filter(|r| r.completed.is_some()).ok_or(ErrorCode::BadState)?;
            return Ok(json!({ "committed": false, "error_code": code, "will_resend": true }));
        }
        let session = c.provider.recovery.take().ok_or(ErrorCode::BadState)?;
        let done = session.completed.ok_or(ErrorCode::BadState)?;
        let staging_dir = c.vault_dir.join(STAGING);
        if status != 200 {
            drop(done);
            let _ = std::fs::remove_dir_all(&staging_dir);
            crate::device::identity::wipe(&c.vault_dir); // the abandoned attempt's identity
            c.state = VaultState::Uninitialized;
            deps.events.emit(ev_state(VaultState::Uninitialized));
            return Ok(json!({ "committed": false, "error_code": code }));
        }
        let generation = body["generation"].as_u64().ok_or(ErrorCode::ManifestMismatch)?;
        let commit = body["state_commit"].as_str().and_then(hex::decode_array::<32>).ok_or(ErrorCode::ManifestMismatch)?;
        publish::committed(&done.store, &done.staging, generation, commit)?;
        let vk = crate::crypto::secret::SecretBytes::new(*done.vk.expose());
        drop(done);
        move_into_place(&staging_dir, &c.vault_dir)?;
        let store = VaultStore::open(&c.vault_dir)?;
        // A recovered vault starts a fresh floor on this Mac (§2.8).
        let _ = super::floor::reset(&store);
        c.header = Some(store.header.clone());
        c.store = Some(store);
        c.vk = Some(vk.mlock_best_effort());
        c.state = VaultState::Unlocked;
        c.note_authorization();
        deps.events.emit(ev_state(VaultState::Unlocked));
        Ok(json!({ "committed": true, "generation": generation }))
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}

/// `header.json` moves last: until it lands, a crash leaves the device
/// UNINITIALIZED (boot sweeps the staging, §13.3) — never a header over a
/// half-moved vault.
fn move_into_place(from: &std::path::Path, to: &std::path::Path) -> Result<(), ErrorCode> {
    for e in std::fs::read_dir(from).map_err(|_| ErrorCode::Internal)? {
        let e = e.map_err(|_| ErrorCode::Internal)?;
        if e.file_name() != crate::VAULT_HEADER_NAME {
            let dest = to.join(e.file_name());
            if dest.is_dir() {
                // A leftover from an earlier interrupted attempt.
                std::fs::remove_dir_all(&dest).map_err(|_| ErrorCode::Internal)?;
            }
            std::fs::rename(e.path(), dest).map_err(|_| ErrorCode::Internal)?;
        }
    }
    let header = crate::VAULT_HEADER_NAME;
    std::fs::rename(from.join(header), to.join(header)).map_err(|_| ErrorCode::Internal)?;
    std::fs::remove_dir(from).map_err(|_| ErrorCode::Internal)
}
