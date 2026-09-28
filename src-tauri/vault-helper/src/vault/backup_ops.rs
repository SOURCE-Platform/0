//! Publication ops (spec v0.4 §1.5, §11.3.2): `backup_prepare` stages a
//! `publish` of the unlocked vault; main lists the blobs, pulls them with
//! `stream_read`, uploads them (each signed through
//! `sign_provider_request`), fetches the body with
//! `backup_transition_body`, posts it, and reports the outcome with
//! `backup_commit_result`. A fully staged publication survives lock.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use vault_proto::b64;

use super::provider_ops::{session_id, PublishSession};
use super::{ev_state, lock_core, Deps, OpOutcome, VaultCore};
use crate::crypto::hex;
use crate::device::SeDevice;
use crate::errors::ErrorCode;
use crate::registry::chain::EpochPolicy;
use crate::registry::log;
use crate::state::VaultState;
use crate::storage::VaultStore;
use crate::sync::publish::{self, Staging};
use crate::sync::session::Transfer;
use crate::sync::{pending, seen};

/// Bodies up to this size travel inline (§1.5 `backup_transition_body`).
const INLINE_BODY: usize = 8 * 1024;
const PAGE: usize = 400;

pub fn transfer_for(st: &Staging) -> Transfer {
    let mut out = st.blobs.clone();
    out.insert(st.body_sha256, st.body.clone());
    Transfer::new(out)
}

pub fn staging_summary(t: &Transfer, st: &Staging) -> Value {
    json!({
        "session": hex::encode(t.id),
        "kind": format!("{:?}", st.kind).to_lowercase(),
        "expected_state": hex::encode(st.expected_state),
        "new_state": hex::encode(st.new_state_commit),
        "generation": st.generation,
        "blob_count": st.blobs.len(),
        "body_sha256": hex::encode(st.body_sha256),
    })
}

/// `backup_prepare`: UNLOCKED → BACKING_UP. A publication already fully
/// staged (e.g. after a failed upload, §11.3.2 retries) is resumed as is —
/// also while LOCKED — rather than refused.
pub fn backup_prepare(core: &Arc<Mutex<VaultCore>>, deps: &Deps) -> OpOutcome {
    let run = || -> Result<Value, ErrorCode> {
        let mut c = lock_core(core);
        if matches!(c.state, VaultState::Unlocked | VaultState::Locked) {
            if let Some(p) = &c.provider.publish {
                return Ok(staging_summary(&p.t, &p.staging));
            }
        }
        if c.state != VaultState::Unlocked || c.provider.sync.is_some() {
            return Err(ErrorCode::BadState);
        }
        let me = SeDevice::load(&c.vault_dir)?;
        let (store, vk) = (c.store.as_ref().ok_or(ErrorCode::Internal)?, c.vk.as_ref().ok_or(ErrorCode::Internal)?);
        let reg = log::read_state(&store.dir, &store.header.vault_id.0, &EpochPolicy::CheckpointAnchored)?;
        let staging = match seen::load(&store.conn)? {
            Some(seen) => {
                let st = publish::stage_publish(store, &reg, vk, &me, &seen, Vec::new())?;
                if publish::unchanged(store, &st)? {
                    return Ok(json!({ "nothing_to_publish": true }));
                }
                st
            }
            // The first `create` never committed (e.g. a restart dropped
            // its staged session): stage it again from the pending record.
            None => restage_create(store, &reg, vk, &me)?,
        };
        let t = transfer_for(&staging);
        let out = staging_summary(&t, &staging);
        c.provider.publish = Some(PublishSession { t, staging });
        deps.events.emit(ev_state(c.reported_state()));
        Ok(out)
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}

/// The vault's first `create`, staged again from the pending
/// `vault_create` record and the stored handle (§11.3.2).
pub fn restage_create(store: &VaultStore, reg: &crate::registry::chain::RegistryState, vk: &crate::crypto::secret::SecretBytes<32>, me: &SeDevice) -> Result<Staging, ErrorCode> {
    let p = pending::load(&store.conn)?.filter(|p| p.ops.contains(&pending::PendingOp::VaultCreate)).ok_or(ErrorCode::BadState)?;
    let handle: String = crate::storage::kv::get(&store.conn, super::rk_ops::HANDLE_KEY)?.ok_or(ErrorCode::BadState)?;
    let seen_auth = seen::Seen { generation: 0, manifest_hash: crate::storage::header::Hex32([0; 32]), state_commit: crate::storage::header::Hex32([0; 32]), recovery_auth: p.recovery_auth_updates };
    let mut st = publish::stage_create(store, reg, vk, me, vault_proto::handle::handle_key(&handle), seen_auth.auth_entries()?)?;
    st.carries_pending = publish::carry_create(store, Some(handle))?;
    Ok(st)
}

/// `backup_blob_list {session, page}` → ≤ 400 `{sha256, size}`.
pub fn backup_blob_list(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    let run = || -> Result<Value, ErrorCode> {
        let id = session_id(frame)?;
        let page = frame.get("page").and_then(Value::as_u64).unwrap_or(0) as usize;
        let c = lock_core(core);
        let st = staged(&c, &id).ok_or(ErrorCode::TransferInvalid)?;
        let items: Vec<Value> = st.blobs.iter().skip(page * PAGE).take(PAGE).map(|(h, b)| json!({"sha256": hex::encode(h), "size": b.len()})).collect();
        Ok(json!({ "blobs": items, "more": st.blobs.len() > (page + 1) * PAGE }))
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}

fn staged<'a>(c: &'a VaultCore, id: &[u8; 16]) -> Option<&'a Staging> {
    if let Some(p) = c.provider.publish.as_ref().filter(|p| &p.t.id == id) {
        return Some(&p.staging);
    }
    let r = c.provider.recovery.as_ref().filter(|r| &r.t.id == id && r.acknowledged)?;
    r.completed.as_ref().map(|d| &d.staging)
}

/// `backup_transition_body {session}` → inline `{body}` (≤ 8 KiB) or
/// `{stream: sha256}` for a larger body (a `create`), read like a blob.
pub fn backup_transition_body(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    let run = || -> Result<Value, ErrorCode> {
        let id = session_id(frame)?;
        let c = lock_core(core);
        let st = staged(&c, &id).ok_or(ErrorCode::TransferInvalid)?;
        Ok(if st.body.len() <= INLINE_BODY {
            json!({ "body": b64::encode(&st.body), "body_sha256": hex::encode(st.body_sha256) })
        } else {
            json!({ "stream": hex::encode(st.body_sha256), "size": st.body.len() })
        })
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}

/// `backup_commit_result {session, status, body}`: `200` → the helper
/// checks the result against the commitment it computed, moves its
/// rollback floor and clears a carried pending change (REMOTE_COMMITTED);
/// `409 STATE_MOVED` → `{sync_required}`; anything else is recorded on the
/// pending change (attempts, last error) for the coordinator's retry.
pub fn backup_commit_result(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let (Ok(id), Some(status)) = (session_id(frame), frame.get("status").and_then(Value::as_u64)) else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    let body: Value = frame.get("body").and_then(Value::as_str).and_then(|s| serde_json::from_str(s).ok()).unwrap_or(Value::Null);
    if lock_core(core).provider.recovery.as_ref().is_some_and(|r| r.t.id == id) {
        return super::recovery_flow::finalize_result(core, status, &body, deps);
    }
    let run = || -> Result<Value, ErrorCode> {
        let mut c = lock_core(core);
        // Only the session reported on is touched (never a newer one).
        if c.provider.publish.as_ref().map(|p| p.t.id) != Some(id) {
            return Err(ErrorCode::TransferInvalid);
        }
        let session = c.provider.publish.take().ok_or(ErrorCode::TransferInvalid)?;
        let mut resend = false;
        // The DB is ciphertext-only here: a fully staged publication can
        // finish while LOCKED by opening the store just for bookkeeping.
        let opened;
        let store = match c.store.as_ref() {
            Some(s) => s,
            None => {
                opened = VaultStore::open(&c.vault_dir)?;
                &opened
            }
        };
        let before = pending::load(&store.conn)?;
        let out = if status == 200 {
            let generation = body["generation"].as_u64().ok_or(ErrorCode::ManifestMismatch)?;
            let commit = body["state_commit"].as_str().and_then(hex::decode_array::<32>).ok_or(ErrorCode::ManifestMismatch)?;
            // §11.3.2 REMOTE_COMMITTED for the version this staging carried;
            // the components cleared drive the UI's post-commit copy.
            let (_, cleared) = publish::committed_clearing(store, &session.staging, generation, commit)?;
            json!({ "committed": true, "generation": generation, "cleared": cleared })
        } else {
            let code = body["error"].as_str().unwrap_or("BACKUP_UNAVAILABLE").to_string();
            if let Some(mut p) = pending::load(&store.conn)? {
                p.attempts = p.attempts.saturating_add(1);
                p.last_error = Some(code.clone());
                pending::save(&store.conn, &p)?;
            }
            // A transient failure keeps the fully staged publication for a
            // byte-identical re-send (also while LOCKED); anything the
            // provider refused on its merits needs a fresh staging.
            resend = transient(status);
            json!({ "committed": false, "sync_required": code == "STATE_MOVED", "error_code": code, "will_resend": resend })
        };
        if before.is_some() {
            if let Ok(ev) = super::remote_status::event(store) {
                deps.events.emit(ev);
            }
        }
        if resend {
            c.provider.publish = Some(session);
        }
        deps.events.emit(ev_state(c.reported_state()));
        Ok(out)
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}

/// Failures worth a byte-identical re-send (§11.2/§11.3.2): unreachable,
/// timeouts, throttling, provider errors, and a GC race on a referenced
/// blob (`412`, re-uploaded by the re-send).
pub fn transient(status: u64) -> bool {
    matches!(status, 0 | 408 | 412 | 429) || status >= 500
}

