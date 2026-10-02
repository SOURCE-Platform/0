//! §22.8 receiver order: canonical parse → vault → receiver → signature
//! (sender resolved in this Mac's local registry) → time → rate → replay
//! → who may speak → body hash. Every failure is an unsigned refusal.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::params;
use vault_proto::peer::{self, PeerOp, PeerRequest};

use super::{Ctx, Refusal};
use crate::registry::chain::EpochPolicy;
use crate::registry::device::DeviceIdentity;
use crate::storage::VaultStore;
use crate::sync::pending;

pub const WINDOW_SECS: u64 = 300;
pub const REPLAY_SECS: u64 = 600;
pub const RATE_PER_MINUTE: usize = 60;
/// Wire annex A.2.2: response bodies per sender in any 10 minutes.
pub const EXCHANGE_BYTES: usize = 64 << 20;
const EXCHANGE_WINDOW: Duration = Duration::from_secs(600);

/// An authenticated request: its prehash binds the response.
pub struct Accepted {
    pub req: PeerRequest,
    pub prehash: [u8; 32],
}

static RATE: Mutex<Option<HashMap<[u8; 16], VecDeque<Instant>>>> = Mutex::new(None);
static SPENT: Mutex<Option<HashMap<[u8; 16], VecDeque<(Instant, usize)>>>> = Mutex::new(None);

/// Charge `bytes` of response body to `sender`; false (nothing charged)
/// when that would pass the exchange cap.
pub fn spend(sender: &[u8; 16], bytes: usize) -> bool {
    let mut guard = SPENT.lock().unwrap_or_else(|e| e.into_inner());
    let q = guard.get_or_insert_with(HashMap::new).entry(*sender).or_default();
    while q.front().is_some_and(|(t, _)| t.elapsed() > EXCHANGE_WINDOW) {
        q.pop_front();
    }
    if q.iter().map(|(_, b)| b).sum::<usize>() + bytes > EXCHANGE_BYTES {
        return false;
    }
    q.push_back((Instant::now(), bytes));
    true
}

/// Counted only after the signature verified, so a forged sender spends
/// nothing (wire annex A.2.2).
fn within_rate(sender: &[u8; 16]) -> bool {
    let mut guard = RATE.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    let q = map.entry(*sender).or_default();
    while q.front().is_some_and(|t| t.elapsed() > Duration::from_secs(60)) {
        q.pop_front();
    }
    if q.len() >= RATE_PER_MINUTE {
        return false;
    }
    q.push_back(Instant::now());
    true
}

/// The envelope checks; `body` is `None` while it is still to be streamed
/// (`peer_serve_begin`), and checked against `body_sha256` otherwise.
pub fn authenticate(ctx: &Ctx, store: &VaultStore, tlv: &[u8], sig: &[u8], body: Option<&[u8]>, now: u64) -> Result<Accepted, Refusal> {
    let req = PeerRequest::decode(tlv).map_err(|_| Refusal::Forbidden)?;
    if req.vault_id != ctx.vault_id || req.receiver_device_id != ctx.me.device_id() {
        return Err(Refusal::Forbidden);
    }
    let reg = crate::registry::log::read_state(&ctx.dir, &ctx.vault_id, &EpochPolicy::CheckpointAnchored).map_err(|_| Refusal::Unavailable)?;
    // Any key the registry ever installed may ask `peer_status` (§22.9);
    // everything else needs an active sender (below).
    let sender = reg.devices.iter().find(|d| d.device_id == req.sender_device_id).ok_or(Refusal::Forbidden)?;
    let prehash = req.prehash();
    peer::verify(&prehash, sig, &sender.sign_pub).map_err(|_| Refusal::Forbidden)?;
    if req.t.abs_diff(now) > WINDOW_SECS {
        return Err(Refusal::Forbidden);
    }
    if !within_rate(&req.sender_device_id) {
        return Err(Refusal::Rate);
    }
    remember(store, &req, now)?;
    may_speak(ctx, store, &req)?;
    if let Some(b) = body {
        if peer::body_hash(b) != req.body_sha256 {
            return Err(Refusal::Forbidden);
        }
    }
    Ok(Accepted { req, prehash })
}

/// §22.8 who may speak, on its own: re-checked when a streamed body
/// completes (a revocation may have landed while it streamed). Any key the
/// registry ever installed may ask `peer_status`; everything else needs a
/// sender that is active and not the target of a pending revocation.
pub fn may_speak(ctx: &Ctx, store: &VaultStore, req: &PeerRequest) -> Result<(), Refusal> {
    if req.operation == PeerOp::Status {
        return Ok(());
    }
    let reg = crate::registry::log::read_state(&ctx.dir, &ctx.vault_id, &EpochPolicy::CheckpointAnchored).map_err(|_| Refusal::Unavailable)?;
    let active = reg.active_device(&req.sender_device_id).is_some();
    let pending_target = pending::revocation_targets(&store.conn).map_err(|_| Refusal::Unavailable)?.contains(&req.sender_device_id);
    if active && !pending_target { Ok(()) } else { Err(Refusal::Forbidden) }
}

/// The persisted replay cache: written before the body is processed,
/// pruned by local receive time beyond 600 s.
fn remember(store: &VaultStore, req: &PeerRequest, now: u64) -> Result<(), Refusal> {
    let c = &store.conn;
    c.execute("DELETE FROM peer_replay WHERE received_at < ?1", params![now.saturating_sub(REPLAY_SECS) as i64]).map_err(|_| Refusal::Unavailable)?;
    let fresh = c
        .execute("INSERT OR IGNORE INTO peer_replay (sender, n, received_at) VALUES (?1, ?2, ?3)", params![&req.sender_device_id[..], &req.n[..], now as i64])
        .map_err(|_| Refusal::Unavailable)?;
    if fresh == 0 { Err(Refusal::Forbidden) } else { Ok(()) }
}

/// Forget the in-memory rate and exchange counts (tests; a helper restart
/// does the same — the caps limit load, not authority).
#[doc(hidden)]
pub fn reset_rate() {
    *RATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *SPENT.lock().unwrap_or_else(|e| e.into_inner()) = None;
}
