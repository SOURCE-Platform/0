//! iPhone remote approval on the Mac (spec §6.5, §6.6; Phase G design
//! `phase-g-design.md`). Pending requests live in helper memory only: a
//! restart forgets them, as it forgets a panel. Each request is shown to
//! the asking iPhone as a §6.5 challenge naming that iPhone; the signed
//! approval that comes back passes §6.6 in order:
//!
//! signature valid for `iphone_device_id` (active iPhone in the committed
//! registry) → `mac_device_id` = this Mac → request pending → origin /
//! action / ref equal → nonce unconsumed → `now ≤ exp` → mark consumed →
//! release.
//!
//! The same approval delivered twice while its request is released is
//! idempotent (one release); after the release was taken it is a replay.

use std::sync::Mutex;

use vault_proto::approval::{result, Action, Challenge, TTL_SECS};

use crate::errors::ErrorCode;
use crate::registry::chain::RegistryState;
use crate::registry::device::PLATFORM_IOS;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Pending,
    /// Approved; the approval's prehash, for idempotent re-delivery.
    Released([u8; 32]),
    Cancelled,
    /// The release was used (the fill went out); its nonce stays spent.
    Taken,
}

struct Request {
    id: [u8; 16],
    origin: String,
    action: Action,
    credential_ref: Option<[u8; 16]>,
    nonce: [u8; 16],
    iat: u64,
    exp: u64,
    state: State,
}

static PENDING: Mutex<Vec<Request>> = Mutex::new(Vec::new());

fn pending() -> std::sync::MutexGuard<'static, Vec<Request>> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner())
}

fn random16() -> Result<[u8; 16], ErrorCode> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|_| ErrorCode::Internal)?;
    Ok(b)
}

/// A request needing the user's presence (Phase H raises these for fills;
/// until then the helper op). Old entries are pruned.
pub fn request(origin: &str, action: Action, credential_ref: Option<[u8; 16]>, now: u64) -> Result<([u8; 16], u64), ErrorCode> {
    if origin.is_empty() || origin.chars().count() > vault_proto::approval::MAX_ORIGIN || credential_ref.is_some() != action.needs_ref() {
        return Err(ErrorCode::InvalidInput);
    }
    let (id, nonce) = (random16()?, random16()?);
    let exp = now + TTL_SECS;
    let mut p = pending();
    p.retain(|r| r.exp + TTL_SECS > now || r.state == State::Taken && r.exp + 600 > now);
    p.push(Request { id, origin: origin.to_string(), action, credential_ref, nonce, iat: now, exp, state: State::Pending });
    Ok((id, exp))
}

/// The challenges `iphone` may answer now (`peer_approvals`).
pub fn challenges_for(mac: [u8; 16], iphone: [u8; 16], now: u64) -> Vec<Challenge> {
    pending()
        .iter()
        .filter(|r| r.state == State::Pending && now <= r.exp)
        .map(|r| Challenge { request_id: r.id, mac_device_id: mac, iphone_device_id: iphone, origin: r.origin.clone(), action: r.action, credential_ref: r.credential_ref, nonce: r.nonce, iat: r.iat, exp: r.exp })
        .collect()
}

/// §6.6 on a signed approval from `sender` (the authenticated peer).
pub fn approve(reg: &RegistryState, mac: [u8; 16], sender: [u8; 16], bytes: &[u8], now: u64) -> u8 {
    let Ok((c, sig)) = vault_proto::approval::decode_signed(bytes) else { return result::MISMATCH };
    // Signature, under an active iPhone that is also the sender.
    let Some(dev) = reg.active_device(&c.iphone_device_id).filter(|d| d.platform == PLATFORM_IOS && d.device_id == sender) else {
        return result::NOT_AUTHORIZED;
    };
    let Ok(prehash) = c.prehash() else { return result::MISMATCH };
    if crate::crypto::ecdsa::verify_prehash(&dev.sign_pub, &prehash, &sig).is_err() {
        return result::SIGNATURE;
    }
    if c.mac_device_id != mac {
        return result::MISMATCH;
    }
    let mut p = pending();
    let Some(r) = p.iter_mut().find(|r| r.id == c.request_id) else { return result::UNKNOWN };
    if r.origin != c.origin || r.action != c.action || r.credential_ref != c.credential_ref || r.iat != c.iat || r.exp != c.exp {
        return result::MISMATCH;
    }
    if r.nonce != c.nonce {
        return result::MISMATCH;
    }
    match r.state {
        State::Released(h) if h == prehash => return result::RELEASED, // duplicate delivery
        State::Released(_) | State::Taken => return result::REPLAY,
        State::Cancelled => return result::CANCELLED,
        State::Pending => {}
    }
    if now > r.exp {
        return result::EXPIRED;
    }
    r.state = State::Released(prehash);
    result::RELEASED
}

/// The phone's unsigned cancel (§6.6): only a pending request.
pub fn cancel(request_id: [u8; 16]) -> u8 {
    match pending().iter_mut().find(|r| r.id == request_id) {
        Some(r) if r.state == State::Pending => {
            r.state = State::Cancelled;
            result::CANCELLED
        }
        Some(_) => result::REPLAY,
        None => result::UNKNOWN,
    }
}

/// `approval_status`: pending / approved / cancelled / expired / taken.
pub fn status(request_id: [u8; 16], now: u64) -> Option<&'static str> {
    pending().iter().find(|r| r.id == request_id).map(|r| match r.state {
        State::Pending if now > r.exp => "expired",
        State::Pending => "pending",
        State::Released(_) => "approved",
        State::Cancelled => "cancelled",
        State::Taken => "taken",
    })
}

/// Use a release exactly once (the fill goes out, Phase H).
pub fn take(request_id: [u8; 16]) -> Result<(), ErrorCode> {
    let mut p = pending();
    match p.iter_mut().find(|r| r.id == request_id) {
        Some(r) if matches!(r.state, State::Released(_)) => {
            r.state = State::Taken;
            Ok(())
        }
        Some(r) if r.state == State::Taken => Err(ErrorCode::ApprovalReplay),
        _ => Err(ErrorCode::BadState),
    }
}

/// Map a `peer_approve` result to the §15 code the phone shows.
pub fn code(r: u8) -> Option<ErrorCode> {
    match r {
        result::RELEASED | result::CANCELLED => None,
        result::REPLAY => Some(ErrorCode::ApprovalReplay),
        result::EXPIRED => Some(ErrorCode::ApprovalExpired),
        result::NOT_AUTHORIZED => Some(ErrorCode::DeviceNotAuthorized),
        result::UNKNOWN => Some(ErrorCode::NotFound),
        result::SIGNATURE => Some(ErrorCode::SignatureInvalid),
        _ => Some(ErrorCode::ApprovalMismatch),
    }
}

/// Tests: forget every pending request.
#[doc(hidden)]
pub fn reset() {
    pending().clear();
}
