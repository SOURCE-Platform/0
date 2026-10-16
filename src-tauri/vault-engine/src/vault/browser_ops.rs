//! The browser ops (spec §9.3, §9.5, §9.6; Phase H) — what the nm-host
//! broker relays for the Chrome extension. Every decision is here:
//!
//! - `browser_hello` → `{vault_state}`.
//! - `browser_candidates {origin, tab_url}` → the page's two origins must
//!   canonicalize to the same one (BR-11); 20 queries a minute (§9.9);
//!   LOCKED → `{locked: true}`; otherwise the eligible logins'
//!   `{ref, title, username}` (metadata only) and a fill request id. A
//!   "similar item" never fills; conflicted or frozen records never fill.
//! - `browser_authorize {request_id, ref, method}` → `local`: presence with
//!   the page's origin named in the prompt, then the one-shot fill;
//!   `iphone`: a §6.5 approval request (`{approval_id}`), then
//!   `browser_fill {request_id}` once the iPhone approved it.
//! - The fill is delivered once (§9.6): the same request again is
//!   `APPROVAL_REPLAY`; it lives 120 s, the fill 30 s in the extension.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::gate::{finish_authorized, presence_gate};
use super::{lock_core, Deps, OpOutcome, VaultCore};
use crate::browser::matching::{best, Verdict};
use crate::browser::origin::{canonical, Origin};
use crate::crypto::hex;
use crate::errors::ErrorCode;
use crate::state::VaultState;
use crate::storage::records::{self, LoginRecord};
use crate::storage::store::now_epoch;

const TTL: u64 = 120;
const QUERIES_PER_MINUTE: usize = 20;

struct Fill {
    id: [u8; 16],
    origin: String,
    refs: Vec<String>,
    created: u64,
    consumed: bool,
    approval: Option<[u8; 16]>,
}

static FILLS: Mutex<Vec<Fill>> = Mutex::new(Vec::new());
static QUERIES: Mutex<Vec<u64>> = Mutex::new(Vec::new());

fn fills() -> std::sync::MutexGuard<'static, Vec<Fill>> {
    FILLS.lock().unwrap_or_else(|e| e.into_inner())
}

fn run(f: impl FnOnce() -> Result<Value, ErrorCode>) -> OpOutcome {
    f().map_or_else(OpOutcome::err, OpOutcome::ok)
}

/// BR-11: the content script's origin and the tab URL must agree.
pub(super) fn page(frame: &Value) -> Result<Origin, ErrorCode> {
    let o = canonical(frame.get("origin").and_then(Value::as_str).ok_or(ErrorCode::InvalidInput)?)?;
    let t = canonical(frame.get("tab_url").and_then(Value::as_str).ok_or(ErrorCode::InvalidInput)?)?;
    if o != t {
        return Err(ErrorCode::ApprovalMismatch);
    }
    Ok(o)
}

fn allow_query(now: u64) -> bool {
    let mut q = QUERIES.lock().unwrap_or_else(|e| e.into_inner());
    q.retain(|t| now.saturating_sub(*t) < 60);
    if q.len() >= QUERIES_PER_MINUTE {
        return false;
    }
    q.push(now);
    true
}

pub fn browser_hello(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    let state = match lock_core(core).state {
        VaultState::Uninitialized => "uninitialized",
        VaultState::Unlocked | VaultState::Authorizing | VaultState::Syncing | VaultState::BackingUp => "unlocked",
        _ => "locked",
    };
    OpOutcome::ok(json!({ "vault_state": state }))
}

pub fn browser_candidates(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let page = page(frame)?;
        let now = now_epoch();
        if !allow_query(now) {
            return Err(ErrorCode::PeerLimit);
        }
        let c = lock_core(core);
        if c.state != VaultState::Unlocked {
            return Ok(json!({ "locked": true }));
        }
        let (store, vk) = (c.store.as_ref().ok_or(ErrorCode::BadState)?, c.vk.as_ref().ok_or(ErrorCode::BadState)?);
        let (mut accounts, mut similar, mut refs) = (Vec::new(), 0, Vec::new());
        for item in store.list_records(vk)? {
            let (Some(r), Some("login")) = (item["ref"].as_str(), item["kind"].as_str()) else { continue };
            if item.get("conflicted").is_some() || item.get("corrupt").is_some() {
                continue; // §3.6: never filled
            }
            let Ok(tip) = store.read_tip(vk, r) else { continue };
            let Ok(rec) = serde_json::from_slice::<LoginRecord>(&tip.plaintext) else { continue };
            match best(&page, &rec.urls) {
                Verdict::Eligible => {
                    accounts.push(json!({ "ref": r, "title": rec.title, "username": rec.username }));
                    refs.push(r.to_string());
                }
                Verdict::Similar => similar += 1,
                Verdict::Refused => {}
            }
        }
        let mut id = [0u8; 16];
        getrandom::fill(&mut id).map_err(|_| ErrorCode::Internal)?;
        let mut f = fills();
        f.retain(|x| now.saturating_sub(x.created) < TTL * 5);
        f.push(Fill { id, origin: page.text(), refs, created: now, consumed: false, approval: None });
        Ok(json!({ "request_id": hex::encode(id), "origin": page.text(), "accounts": accounts, "similar": similar }))
    })
}

/// The fill request named by the frame, checked: this ref, unexpired,
/// unused. Returns its origin.
fn check(id: &[u8; 16], r: Option<&str>, now: u64) -> Result<(String, Option<[u8; 16]>), ErrorCode> {
    let f = fills();
    let x = f.iter().find(|x| &x.id == id).ok_or(ErrorCode::NotFound)?;
    if x.consumed {
        return Err(ErrorCode::ApprovalReplay);
    }
    if now.saturating_sub(x.created) > TTL {
        return Err(ErrorCode::ApprovalExpired);
    }
    if r.is_some_and(|r| !x.refs.iter().any(|s| s == r)) {
        return Err(ErrorCode::ApprovalMismatch);
    }
    Ok((x.origin.clone(), x.approval))
}

fn consume(id: &[u8; 16], r: &str) -> Result<(), ErrorCode> {
    let mut f = fills();
    let x = f.iter_mut().find(|x| &x.id == id).ok_or(ErrorCode::NotFound)?;
    if x.consumed {
        return Err(ErrorCode::ApprovalReplay);
    }
    x.consumed = true;
    x.refs.retain(|s| s == r);
    Ok(())
}

/// The fill and the origin it was approved for: the extension delivers it
/// only to a tab still showing exactly that origin.
fn the_fill(store: &crate::storage::VaultStore, vk: &crate::crypto::secret::SecretBytes<32>, r: &str, origin: &str) -> Result<Value, ErrorCode> {
    let tip = store.read_tip(vk, r)?;
    if tip.kind_tag != records::KIND_LOGIN {
        return Err(ErrorCode::InvalidInput);
    }
    let rec: LoginRecord = serde_json::from_slice(&tip.plaintext).map_err(|_| ErrorCode::RecordCorrupt)?;
    Ok(json!({ "fill": { "username": rec.username, "password": rec.password }, "origin": origin, "expires_in": 30 }))
}

pub fn browser_authorize(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let parsed = (|| {
        let id = frame.get("request_id").and_then(Value::as_str).and_then(hex::decode_array::<16>).ok_or(ErrorCode::InvalidInput)?;
        let r = frame.get("ref").and_then(Value::as_str).ok_or(ErrorCode::InvalidInput)?.to_string();
        let (origin, _) = check(&id, Some(&r), now_epoch())?;
        Ok::<_, ErrorCode>((id, r, origin))
    })();
    let (id, r, origin) = match parsed {
        Ok(v) => v,
        Err(e) => return OpOutcome::err(e),
    };
    match frame.get("method").and_then(Value::as_str) {
        Some("local") => {
            // §9.9: the prompt — rendered by the OS, not the extension —
            // names the origin the fill goes to.
            if let Err(o) = presence_gate(core, deps, &format!("Source Vault: fill your password on {origin}")) {
                return o;
            }
            finish_authorized!(core, deps, move |store: &mut crate::storage::VaultStore, vk: &crate::crypto::secret::SecretBytes<32>| {
                consume(&id, &r)?;
                the_fill(store, vk, &r, &origin)
            })
        }
        Some("iphone") => run(|| {
            let rid = crate::storage::revisions::uuid_bytes(&r).ok_or(ErrorCode::InvalidInput)?;
            let (approval, exp) = crate::approval::request(&origin, vault_proto::approval::Action::FillPassword, Some(rid), now_epoch())?;
            if let Some(x) = fills().iter_mut().find(|x| x.id == id) {
                x.approval = Some(approval);
                x.refs.retain(|s| *s == r);
            }
            deps.events.emit(json!({ "event": "approval_pending", "request_id": hex::encode(approval) }));
            Ok(json!({ "approval_id": hex::encode(approval), "exp": exp }))
        }),
        _ => OpOutcome::err(ErrorCode::InvalidInput),
    }
}

/// After the iPhone approved (Phase G): the fill, once.
pub fn browser_fill(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let id = frame.get("request_id").and_then(Value::as_str).and_then(hex::decode_array::<16>).ok_or(ErrorCode::InvalidInput)?;
        let (origin, approval) = check(&id, None, now_epoch())?;
        let approval = approval.ok_or(ErrorCode::BadState)?;
        match crate::approval::status(approval, now_epoch()) {
            Some("approved") => {}
            Some("pending") => return Ok(json!({ "pending": true })),
            Some("expired") => return Err(ErrorCode::ApprovalExpired),
            _ => return Err(ErrorCode::PresenceDenied),
        }
        let c = lock_core(core);
        if c.state != VaultState::Unlocked {
            return Err(ErrorCode::BadState);
        }
        let r = fills().iter().find(|x| x.id == id).and_then(|x| x.refs.first().cloned()).ok_or(ErrorCode::NotFound)?;
        crate::approval::take(approval)?;
        consume(&id, &r)?;
        the_fill(c.store.as_ref().ok_or(ErrorCode::BadState)?, c.vk.as_ref().ok_or(ErrorCode::BadState)?, &r, &origin)
    })
}

/// Tests: forget every fill request and query count.
#[doc(hidden)]
pub fn reset() {
    fills().clear();
    QUERIES.lock().unwrap_or_else(|e| e.into_inner()).clear();
}
