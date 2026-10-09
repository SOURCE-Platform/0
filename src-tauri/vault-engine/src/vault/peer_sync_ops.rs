//! The phone's peer sync ops (spec v0.5 §22.8; plan
//! `phase-f2c-phone-plan.md`): SOURCE Vault carries bytes, the engine does
//! everything else.
//!
//! - `peer_sync_begin {}` → `{request, endpoint}`: the first signed request
//!   (HTTP carriage entry, base64url) and where to send it — the paired
//!   Mac's host hints, port, pin and bearer token (the token authorizes
//!   nothing in the vault).
//! - `peer_sync_step {response}` → `{request}` | `{done}` | `{removed}`;
//!   `peer_sync_step {refused: <http status>}` ends the exchange.
//!
//! UNLOCKED only (admission opens revisions under the VK); one exchange at
//! a time; a lock ends it. A verified removal locks the vault (§22.9).

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use vault_proto::b64;
use vault_proto::crypto::registry::EntryKind;

use super::{lock_core, Deps, LockReason, OpOutcome, VaultCore};
use crate::device::SeDevice;
use crate::errors::ErrorCode;
use crate::peer::client::{removal, Env, Exchange, Step};
use crate::registry::chain::{EpochPolicy, RegistryState};
use crate::state::VaultState;
use crate::storage::store::now_epoch;

/// The Mac this phone pairs with: the device that authorized its enroll
/// entry, still active in the committed registry.
fn paired_mac(reg: &RegistryState, me: &[u8; 16]) -> Result<[u8; 16], ErrorCode> {
    let mac = reg.entries.iter().find(|e| e.kind == EntryKind::Enroll && &e.device_id == me).and_then(|e| e.authorizer).ok_or(ErrorCode::PeerNotPermitted)?;
    reg.active_device(&mac).map(|d| d.device_id).ok_or(ErrorCode::PeerNotPermitted)
}

/// Run one exchange step with the unlocked engine's parts.
fn with_env<T>(c: &mut VaultCore, f: impl FnOnce(&mut Env<'_>, &mut Option<Exchange>) -> Result<T, ErrorCode>) -> Result<T, ErrorCode> {
    if c.state != VaultState::Unlocked || c.behind || c.unverified_key {
        return Err(ErrorCode::BadState);
    }
    if removal::load(&c.vault_dir).is_some() {
        return Err(ErrorCode::PeerNotPermitted);
    }
    let me = SeDevice::load(&c.vault_dir)?;
    let vk = c.vk.as_ref().ok_or(ErrorCode::BadState)?;
    let vk = crate::crypto::secret::SecretBytes::new(*vk.expose());
    let store = c.store.as_mut().ok_or(ErrorCode::BadState)?;
    let committed = crate::registry::log::read_state(&c.vault_dir, &store.header.vault_id.0, &EpochPolicy::CheckpointAnchored)?;
    let manifest_hash = crate::sync::seen::load(&store.conn)?.map(|s| s.manifest_hash.0);
    let mut exchange = c.provider.peer_client.take();
    let mut env = Env { store, vk: &vk, me: &me, committed: &committed, manifest_hash, now: now_epoch() };
    let out = f(&mut env, &mut exchange);
    c.provider.peer_client = exchange;
    out
}

pub fn peer_sync_begin(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    let mut c = lock_core(core);
    let endpoint = match super::join_ops::stored_peer_endpoint() {
        Ok(Some(ep)) => ep,
        Ok(None) => return OpOutcome::err(ErrorCode::BadState), // not paired
        Err(e) => return OpOutcome::err(e),
    };
    let vault_id = match c.store.as_ref() {
        Some(s) => s.header.vault_id.0,
        None => return OpOutcome::err(ErrorCode::BadState),
    };
    let out = with_env(&mut c, |env, slot| {
        let mac = paired_mac(env.committed, &env.me.device_id())?;
        let (exchange, request) = Exchange::begin(env, vault_id, mac)?;
        *slot = Some(exchange);
        Ok(json!({ "request": b64::encode(&request), "endpoint": endpoint }))
    });
    out.map_or_else(OpOutcome::err, OpOutcome::ok)
}

pub fn peer_sync_step(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let mut c = lock_core(core);
    if let Some(code) = frame.get("refused").and_then(Value::as_u64) {
        // Unsigned refusals end the exchange: "unable to verify", or the
        // per-sender rate (annex A.2.1).
        c.provider.peer_client = None;
        return OpOutcome::err(if code == 429 { ErrorCode::PeerLimit } else { ErrorCode::PeerAuthInvalid });
    }
    let Some(response) = frame.get("response").and_then(Value::as_str).and_then(b64::decode) else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    let step = with_env(&mut c, |env, slot| {
        let exchange = slot.as_mut().ok_or(ErrorCode::BadState)?;
        let step = exchange.step(env, &response);
        if !matches!(step, Ok(Step::Request(_))) {
            *slot = None; // done, removed, or unable to verify: the exchange ends
        }
        step
    });
    match step {
        Ok(Step::Request(r)) => OpOutcome::ok(json!({ "request": b64::encode(&r) })),
        Ok(Step::Done(summary)) => OpOutcome::ok(json!({ "done": summary.json() })),
        Ok(Step::Removed { published }) => {
            let recorded = removal::record(&c.vault_dir, published, now_epoch());
            for ev in c.lock(LockReason::Explicit) {
                deps.events.emit(ev);
            }
            match recorded {
                Ok(r) => OpOutcome::ok(json!({ "removed": { "published": r.published } })),
                Err(e) => OpOutcome::err(e),
            }
        }
        Err(e) => OpOutcome::err(e),
    }
}
