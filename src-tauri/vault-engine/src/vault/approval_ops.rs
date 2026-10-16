//! iPhone remote approval ops (spec §6.5, §6.6, §7.1; Phase G design
//! `phase-g-design.md`).
//!
//! Mac helper: `approval_request {origin, action, credential_ref?}` →
//! `{request_id, exp}`; `approval_status {request_id}` → `{state}`;
//! `approval_take {request_id}` uses a release once (Phase H's fill).
//!
//! iPhone engine (LOCKED or UNLOCKED — the signing key, not the vault key,
//! is what signs; not while removed):
//! `approvals_fetch_begin` → `{request, endpoint}` (a signed `peer_approvals`);
//! `approvals_fetch_step {response}` → the verified challenges;
//! `approval_decide {request_id, approve}` → presence for an approval, then
//! the signed TLV (or a cancel) in a `peer_approve` → `{request}`;
//! `approval_submit_step {response}` → `{result}`.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use vault_proto::approval::{self, Action, Challenge};
use vault_proto::b64;
use vault_proto::peer::{PeerOp, PeerStatus};

use super::{lock_core, Deps, OpOutcome, VaultCore};
use crate::crypto::hex;
use crate::device::identity::SeDevice;
use crate::errors::ErrorCode;
use crate::peer::client::envelope::{self, Outstanding};
use crate::registry::chain::{EpochPolicy, RegistryState};
use crate::registry::device::DeviceIdentity;
use crate::state::VaultState;
use crate::storage::store::now_epoch;

/// The phone's approval exchange: one request in flight, the challenges
/// it verified.
#[derive(Default)]
pub struct PhoneApproval {
    out: Option<(Outstanding, Option<[u8; 16]>)>,
    seen: Vec<Challenge>,
}

fn run(f: impl FnOnce() -> Result<Value, ErrorCode>) -> OpOutcome {
    f().map_or_else(OpOutcome::err, OpOutcome::ok)
}

fn id16(frame: &Value, key: &str) -> Result<[u8; 16], ErrorCode> {
    frame.get(key).and_then(Value::as_str).and_then(hex::decode_array::<16>).ok_or(ErrorCode::InvalidInput)
}

// --- Mac -------------------------------------------------------------------

pub fn approval_request(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        if lock_core(core).phone {
            return Err(ErrorCode::UnknownOp);
        }
        let origin = frame.get("origin").and_then(Value::as_str).ok_or(ErrorCode::InvalidInput)?;
        let action = frame.get("action").and_then(Value::as_str).and_then(Action::from_name).ok_or(ErrorCode::InvalidInput)?;
        let r = frame.get("credential_ref").and_then(Value::as_str).map(|s| hex::decode_array::<16>(&s.replace('-', "")).ok_or(ErrorCode::InvalidInput)).transpose()?;
        let (id, exp) = crate::approval::request(origin, action, r, now_epoch())?;
        Ok(json!({ "request_id": hex::encode(id), "exp": exp }))
    })
}

pub fn approval_status(frame: &Value) -> OpOutcome {
    run(|| {
        let state = crate::approval::status(id16(frame, "request_id")?, now_epoch()).ok_or(ErrorCode::NotFound)?;
        Ok(json!({ "state": state }))
    })
}

pub fn approval_take(frame: &Value) -> OpOutcome {
    run(|| crate::approval::take(id16(frame, "request_id")?).map(|_| json!({})))
}

// --- iPhone ----------------------------------------------------------------

/// Who and where: this phone, its vault, its registry, the Mac it asks.
fn parts(c: &VaultCore) -> Result<(SeDevice, [u8; 16], RegistryState, [u8; 16], Value), ErrorCode> {
    if !c.phone || !matches!(c.state, VaultState::Locked | VaultState::Unlocked) {
        return Err(ErrorCode::BadState);
    }
    if crate::peer::client::removal::active(true, &c.vault_dir).is_some() {
        return Err(ErrorCode::PeerNotPermitted);
    }
    let me = SeDevice::load(&c.vault_dir)?;
    let vid = c.header.as_ref().ok_or(ErrorCode::BadState)?.vault_id.0;
    let reg = crate::registry::log::read_state(&c.vault_dir, &vid, &EpochPolicy::CheckpointAnchored)?;
    let endpoint = super::join_ops::stored_peer_endpoint()?.ok_or(ErrorCode::BadState)?;
    let mac = super::peer_sync_ops::target_mac(&reg, &me.device_id(), &endpoint)?;
    Ok((me, vid, reg, mac, endpoint))
}

pub fn approvals_fetch_begin(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    run(|| {
        let mut c = lock_core(core);
        let (me, vid, _, mac, endpoint) = parts(&c)?;
        let (bytes, out) = envelope::request(&me, vid, mac, PeerOp::Approvals, &vault_proto::peer::body::empty(), now_epoch())?;
        c.provider.approval = Some(PhoneApproval { out: Some((out, None)), seen: Vec::new() });
        Ok(json!({ "request": b64::encode(&bytes), "endpoint": endpoint }))
    })
}

/// The Mac's signed answer: the challenges naming this phone and that Mac.
pub fn approvals_fetch_step(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let bytes = frame.get("response").and_then(Value::as_str).and_then(b64::decode).ok_or(ErrorCode::InvalidInput)?;
        let mut c = lock_core(core);
        let (me, vid, reg, mac, _) = parts(&c)?;
        let a = c.provider.approval.as_mut().ok_or(ErrorCode::BadState)?;
        let (out, None) = a.out.take().ok_or(ErrorCode::BadState)? else { return Err(ErrorCode::BadState) };
        let (status, body) = envelope::response(&reg, vid, me.device_id(), &out, &bytes)?;
        if status != PeerStatus::Ok || out.op != PeerOp::Approvals {
            return Err(ErrorCode::PeerAuthInvalid);
        }
        let now = now_epoch();
        a.seen = approval::decode_list(&body)?.into_iter().filter(|ch| ch.iphone_device_id == me.device_id() && ch.mac_device_id == mac && now <= ch.exp).collect();
        let list: Vec<Value> = a.seen.iter().map(|ch| json!({ "request_id": hex::encode(ch.request_id), "origin": ch.origin, "action": ch.action.name(), "credential_ref": ch.credential_ref.map(hex::encode), "exp": ch.exp })).collect();
        Ok(json!({ "approvals": list }))
    })
}

/// Approve (presence, then sign exactly the verified challenge) or cancel.
pub fn approval_decide(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    run(|| {
        let id = id16(frame, "request_id")?;
        let yes = frame.get("approve").and_then(Value::as_bool).ok_or(ErrorCode::InvalidInput)?;
        let ch = lock_core(core).provider.approval.as_ref().and_then(|a| a.seen.iter().find(|ch| ch.request_id == id).cloned()).ok_or(ErrorCode::NotFound)?;
        let body = if yes {
            if now_epoch() > ch.exp {
                return Err(ErrorCode::ApprovalExpired);
            }
            if !deps.la.check(&format!("Approve: {} on {}", ch.action.name().replace('_', " "), ch.origin)) {
                return Err(ErrorCode::PresenceDenied);
            }
            let me = SeDevice::load(&lock_core(core).vault_dir)?;
            let sig = me.sign_prehash(&ch.prehash()?).map_err(|_| ErrorCode::DeviceNotAuthorized)?;
            ch.signed(&sig)?
        } else {
            approval::encode_cancel(&id)
        };
        let mut c = lock_core(core);
        let (me, vid, _, mac, endpoint) = parts(&c)?;
        let (bytes, out) = envelope::request(&me, vid, mac, PeerOp::Approve, &body, now_epoch())?;
        let a = c.provider.approval.as_mut().ok_or(ErrorCode::BadState)?;
        a.out = Some((out, Some(id)));
        Ok(json!({ "request": b64::encode(&bytes), "endpoint": endpoint }))
    })
}

pub fn approval_submit_step(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let bytes = frame.get("response").and_then(Value::as_str).and_then(b64::decode).ok_or(ErrorCode::InvalidInput)?;
        let mut c = lock_core(core);
        let (me, vid, reg, _, _) = parts(&c)?;
        let a = c.provider.approval.as_mut().ok_or(ErrorCode::BadState)?;
        let (out, Some(id)) = a.out.take().ok_or(ErrorCode::BadState)? else { return Err(ErrorCode::BadState) };
        let (status, body) = envelope::response(&reg, vid, me.device_id(), &out, &bytes)?;
        let [r] = body.as_slice() else { return Err(ErrorCode::PeerAuthInvalid) };
        if status != PeerStatus::Ok || out.op != PeerOp::Approve {
            return Err(ErrorCode::PeerAuthInvalid);
        }
        a.seen.retain(|ch| ch.request_id != id);
        match crate::approval::code(*r) {
            None => Ok(json!({ "result": if *r == approval::result::CANCELLED { "cancelled" } else { "approved" } })),
            Some(e) => Err(e),
        }
    })
}

// --- Push (§7.2) -----------------------------------------------------------

fn push_out(origin: &str, pr: &vault_proto::request::ProviderRequest, auth: String, body: Vec<u8>) -> Value {
    json!({ "origin": origin, "method": pr.method, "path": pr.path, "auth": auth, "body": String::from_utf8_lossy(&body) })
}

/// iPhone: `push_register_request {token}` → the signed request that
/// registers this phone's APNs token with the vault's provider.
pub fn push_register_request(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let token = frame.get("token").and_then(Value::as_str).filter(|t| !t.is_empty() && t.len() <= 200 && hex::decode(t).is_some()).ok_or(ErrorCode::InvalidInput)?;
        let c = lock_core(core);
        if !c.phone {
            return Err(ErrorCode::UnknownOp);
        }
        let h = c.header.as_ref().ok_or(ErrorCode::BadState)?;
        let me = SeDevice::load(&c.vault_dir)?;
        let (pr, auth, body) = crate::sync::sign::sign_push(&h.provider, h.vault_id.0, &me, &json!({ "token": token }), vault_proto::request::Operation::PushRegister, now_epoch())?;
        Ok(push_out(&h.provider, &pr, auth, body))
    })
}

/// Mac: `push_wake_request {request_id}` → one signed wake per active
/// iPhone of the vault, only for a request still pending.
pub fn push_wake_request(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let id = id16(frame, "request_id")?;
        if crate::approval::status(id, now_epoch()) != Some("pending") {
            return Err(ErrorCode::BadState);
        }
        let c = lock_core(core);
        if c.phone {
            return Err(ErrorCode::UnknownOp);
        }
        let h = c.header.as_ref().ok_or(ErrorCode::BadState)?;
        let me = SeDevice::load(&c.vault_dir)?;
        let reg = crate::registry::log::read_state(&c.vault_dir, &h.vault_id.0, &EpochPolicy::CheckpointAnchored)?;
        let mut out = Vec::new();
        for d in reg.devices.iter().filter(|d| !d.revoked && d.platform == crate::registry::device::PLATFORM_IOS && d.device_id != me.device_id()) {
            let body = json!({ "to": hex::encode(d.device_id), "rid": hex::encode(id) });
            let (pr, auth, bytes) = crate::sync::sign::sign_push(&h.provider, h.vault_id.0, &me, &body, vault_proto::request::Operation::PushSend, now_epoch())?;
            let mut v = push_out(&h.provider, &pr, auth, bytes);
            v["device_id"] = json!(hex::encode(d.device_id));
            out.push(v);
        }
        Ok(json!({ "requests": out }))
    })
}
