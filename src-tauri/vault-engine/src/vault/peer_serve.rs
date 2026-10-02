//! `peer_serve` / `peer_serve_begin` (spec v0.5 §1.5, §22.8; wire annex
//! A.2.2): one signed peer request, relayed by main as opaque bytes. The
//! helper verifies, answers and signs everything itself.
//!
//! - Inline: `peer_serve {request_tlv, signature, body}` (body ≤ 24 KiB).
//! - Large: `peer_serve_begin {request_tlv, signature, size}` runs the
//!   whole receiver order before accepting a byte — or answers at once
//!   (refusal, or a signed status 1/2) — then the body streams in through
//!   the §1.3 stream ops of that peer session, and `peer_serve {session}`
//!   re-checks who may speak, checks the body hash and consumes it.
//! - A response body over 24 KiB is a one-blob stream session.
//!
//! Served while LOCKED, UNLOCKED, BACKING_UP, SYNCING and COMPROMISED;
//! refusals are `{refused: 403 | 429 | 503}`, unsigned.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use vault_proto::b64;
use vault_proto::peer::{PeerOp, PeerStatus};

use super::provider_ops::MAX_PEER_SESSIONS;
use super::{lock_core, OpOutcome, VaultCore};
use crate::crypto::hex;
use crate::device::SeDevice;
use crate::errors::ErrorCode;
use crate::peer::respond::{status_only, Signed};
use crate::peer::{ops, verify, Ctx, Refusal};
use crate::state::VaultState;
use crate::storage::store::now_epoch;
use crate::storage::VaultStore;
use crate::sync::session::Transfer;

pub const INLINE: usize = 24 * 1024;
pub const MAX_BODY: u64 = 1 << 20;
const FRESH: Duration = Duration::from_secs(15 * 60);

/// When the provider state was last verified, on both clocks: the
/// monotonic one stops while the Mac sleeps (review VER-I7), the wall
/// clock can be set back; fresh only while both say so.
#[derive(Clone, Copy)]
pub struct Checked {
    at: std::time::Instant,
    wall: u64,
}

impl Checked {
    pub fn now() -> Checked {
        Checked { at: std::time::Instant::now(), wall: now_epoch() }
    }

    fn fresh(&self) -> bool {
        let wall = now_epoch();
        self.at.elapsed() < FRESH && wall >= self.wall && wall - self.wall < FRESH.as_secs()
    }
}

fn field(frame: &Value, k: &str) -> Result<Vec<u8>, ErrorCode> {
    frame.get(k).and_then(Value::as_str).and_then(b64::decode).ok_or(ErrorCode::InvalidInput)
}

fn refused(r: Refusal) -> Value {
    json!({ "refused": r.http() })
}

/// The serving context for this request, or "not serving" (503).
fn context(c: &VaultCore, store: &VaultStore) -> Result<Ctx, Refusal> {
    let serving = matches!(c.state, VaultState::Locked | VaultState::Unlocked | VaultState::BackingUp | VaultState::Syncing | VaultState::Compromised);
    if !serving || c.unverified_key {
        return Err(Refusal::Unavailable);
    }
    let me = SeDevice::load(&c.vault_dir).map_err(|_| Refusal::Unavailable)?;
    let locked = c.vk.is_none();
    // COMPROMISED is persisted evidence: a LOCKED Mac reads it too (review
    // SEC-O3), failing closed when it cannot.
    let compromised = c.state == VaultState::Compromised || !matches!(crate::storage::compromised::load(&store.conn), Ok(None));
    // The floor check needs no key, so a LOCKED Mac applies it too.
    let behind = if locked { crate::vault::floor::behind(store).unwrap_or(true) } else { c.behind };
    Ok(Ctx {
        dir: c.vault_dir.clone(),
        vault_id: store.header.vault_id.0,
        me,
        behind,
        compromised,
        locked,
        fresh: !locked && c.provider_checked.is_some_and(|t| t.fresh()),
    })
}

/// Run `f` with the context and the resident store (or one opened just
/// for this request while LOCKED).
fn with_store(core: &Arc<Mutex<VaultCore>>, f: impl FnOnce(&mut VaultCore, &Ctx, &mut VaultStore) -> Result<Value, ErrorCode>) -> OpOutcome {
    let run = || -> Result<Value, ErrorCode> {
        let mut guard = lock_core(core);
        let c = &mut *guard;
        let mut opened = match c.store.is_some() {
            true => None,
            false => Some(VaultStore::open(&c.vault_dir)?),
        };
        let ctx = {
            let store = opened.as_ref().or(c.store.as_ref()).ok_or(ErrorCode::Internal)?;
            match context(c, store) {
                Ok(ctx) => ctx,
                Err(r) => return Ok(refused(r)),
            }
        };
        let mut resident = c.store.take();
        let store = opened.as_mut().or(resident.as_mut()).ok_or(ErrorCode::Internal)?;
        let out = f(c, &ctx, store);
        c.store = resident;
        out
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}

/// The helper's signed answer: inline, or a one-blob stream session — or,
/// with both peer sessions taken, the signed status 2 (annex A.2.2).
fn answer(c: &mut VaultCore, ctx: &Ctx, acc: &verify::Accepted, signed: Signed) -> Value {
    if signed.body.len() > INLINE && c.provider.peer_sessions() >= MAX_PEER_SESSIONS {
        return match status_only(ctx, acc, PeerStatus::Limit, now_epoch()) {
            Ok(s) => answer(c, ctx, acc, s),
            Err(r) => refused(r),
        };
    }
    let mut out = json!({
        "response_tlv": b64::encode(&signed.response_tlv),
        "signature": b64::encode(&signed.signature),
    });
    if signed.body.len() <= INLINE {
        out["body"] = json!(b64::encode(&signed.body));
    } else {
        let sha = vault_proto::peer::body_hash(&signed.body);
        let size = signed.body.len();
        let t = Transfer::new(BTreeMap::from([(sha, signed.body)]));
        out["session"] = json!(hex::encode(t.id));
        out["stream"] = json!(hex::encode(sha));
        out["size"] = json!(size);
        c.provider.peer.push(t);
    }
    out
}

fn serve_one(c: &mut VaultCore, ctx: &Ctx, store: &mut VaultStore, acc: &verify::Accepted, body: &[u8], now: u64) -> Value {
    let vk = c.vk.as_ref().map(|v| crate::crypto::secret::SecretBytes::new(*v.expose()));
    match ops::serve(ctx, store, vk.as_ref(), acc, body, now) {
        Ok(signed) => answer(c, ctx, acc, signed),
        Err(r) => refused(r),
    }
}

pub fn peer_serve(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    if frame.get("session").is_some() {
        return complete(core, frame);
    }
    let parsed = (field(frame, "request_tlv"), field(frame, "signature"), field(frame, "body"));
    let (Ok(tlv), Ok(sig), Ok(body)) = parsed else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    if body.len() > INLINE {
        return OpOutcome::err(ErrorCode::InvalidInput); // use peer_serve_begin
    }
    with_store(core, |c, ctx, store| {
        let now = now_epoch();
        Ok(match verify::authenticate(ctx, store, &tlv, &sig, Some(&body), now) {
            Ok(acc) => serve_one(c, ctx, store, &acc, &body, now),
            Err(r) => refused(r),
        })
    })
}

/// `peer_serve_begin {request_tlv, signature, size}`.
pub fn peer_serve_begin(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    let (Ok(tlv), Ok(sig), Some(size)) = (field(frame, "request_tlv"), field(frame, "signature"), frame.get("size").and_then(Value::as_u64)) else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    with_store(core, |c, ctx, store| {
        let now = now_epoch();
        let acc = match verify::authenticate(ctx, store, &tlv, &sig, None, now) {
            Ok(a) => a,
            Err(r) => return Ok(refused(r)),
        };
        let op = acc.req.operation;
        let gated = (ctx.behind && !matches!(op, PeerOp::Hello | PeerOp::Status)) || (ctx.compromised && op == PeerOp::RevsPut);
        let early = if gated {
            Some(PeerStatus::BadState)
        } else if size > MAX_BODY || c.provider.peer_sessions() >= MAX_PEER_SESSIONS {
            Some(PeerStatus::Limit) // over the cap, or both peer sessions taken
        } else {
            None
        };
        if let Some(st) = early {
            return Ok(match status_only(ctx, &acc, st, now) {
                Ok(signed) => answer(c, ctx, &acc, signed),
                Err(r) => refused(r),
            });
        }
        let mut t = Transfer::new(BTreeMap::new());
        t.expect([(acc.req.body_sha256, MAX_BODY)]);
        let out = json!({ "session": hex::encode(t.id), "need": [hex::encode(acc.req.body_sha256)] });
        c.provider.peer_in.push((acc, t));
        Ok(out)
    })
}

/// `peer_serve {session}`: the streamed body arrived. Single use.
fn complete(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    let Some(id) = frame.get("session").and_then(Value::as_str).and_then(hex::decode_array::<16>) else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    with_store(core, |c, ctx, store| {
        let Some(i) = c.provider.peer_in.iter().position(|(_, t)| t.id == id) else {
            return Ok(refused(Refusal::Forbidden));
        };
        let (acc, t) = c.provider.peer_in.swap_remove(i);
        let Some(body) = t.received.get(&acc.req.body_sha256).cloned() else {
            return Ok(refused(Refusal::Forbidden)); // never streamed, or failed
        };
        // A revocation may have landed while the body streamed.
        if let Err(r) = verify::may_speak(ctx, store, &acc.req) {
            return Ok(refused(r));
        }
        if vault_proto::peer::body_hash(&body) != acc.req.body_sha256 {
            return Ok(refused(Refusal::Forbidden));
        }
        Ok(serve_one(c, ctx, store, &acc, &body, now_epoch()))
    })
}
