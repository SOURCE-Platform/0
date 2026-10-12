//! The phone's provider path (spec v0.5 §11.1, F.2d step 2; plan
//! `phase-f2d-plan.md` revision 2): `vault-coordinator`'s flows run inside
//! one `ov0_engine_call` (`provider_sync`, `provider_publish`, and the
//! total-loss recovery `provider_recover_start` / `provider_recover_finish`),
//! with
//!
//! - `Helper` = the engine's own dispatcher, limited to `SUB_OPS`, each
//!   frame held to `MAX_REQUEST`, events flushed between sub-ops — and the
//!   op lane is never taken again (it is held by this very call);
//! - `Transport` = Swift's `http_send`, only to the vault header's own
//!   provider origin (for recovery, with no vault yet, the engine's
//!   allowlisted default), each answer capped at `MAX_ANSWER`, 30 s each,
//!   refused once `ov0_engine_close` has set the cancel flag.
//!
//! A callback that calls back into the engine meanwhile gets `BAD_STATE`
//! (`in_provider`). The coordinator holds no secret (§11.1); everything it
//! moves is ciphertext, public data or a request the engine signed.
//! The §11.3.2 access-loss count is kept in the engine's store, since the
//! app's lifetimes are too short for a count held in memory (SPEC-I3).

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{json, Value};
use vault_coordinator::flows::Flows;
use vault_coordinator::policy::{AccessSignal, AccessWatch};
use vault_coordinator::{Failure, Helper, HttpResponse, Transport, TransportError};
use vault_engine::errors::ErrorCode;
use vault_engine::peer::client::removal;
use vault_engine::state::VaultState;
use vault_engine::storage::header::default_provider;
use vault_engine::storage::kv;
use vault_engine::vault::{dispatch, lock_core, OpOutcome};

use crate::callbacks::HttpOutcome;
use crate::engine::{Inner, MAX_REQUEST};

/// The engine ops a provider flow may run (a subset of what the Mac's
/// coordinator uses; recovery since F.2d chunk 3).
pub const SUB_OPS: &[&str] = &[
    "get_state", "backup_prepare", "backup_blob_list", "backup_transition_body", "backup_commit_result", "backup_state_offer",
    "backup_apply", "stream_begin", "stream_write", "stream_end", "stream_cancel", "stream_read", "sign_provider_request",
    "session_close", "remote_update_status",
    // F.2d chunk 3: total-loss recovery onto this phone (§11.5, §11.8).
    "recovery_begin", "recovery_preview", "recovery_complete",
];

/// §22.8 / §11.2: no provider answer the engine takes is larger.
pub const MAX_ANSWER: usize = (8 << 20) + 4096;
const TIMEOUT_MS: u64 = 30_000;
const WATCH_KEY: &str = "phone_access_watch";
pub const ACCESS_LOST_KEY: &str = "phone_access_lost";

thread_local! {
    static IN_PROVIDER: Cell<bool> = const { Cell::new(false) };
}

/// A provider flow is running on this thread (re-entrancy guard).
pub fn in_provider() -> bool {
    IN_PROVIDER.with(Cell::get)
}

struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        IN_PROVIDER.with(|f| f.set(false));
    }
}

struct EngineHelper<'a>(&'a Inner);

impl Helper for EngineHelper<'_> {
    fn op(&self, frame: Value) -> Result<Value, String> {
        let op = frame.get("op").and_then(Value::as_str).unwrap_or("");
        if !SUB_OPS.contains(&op) {
            return Ok(OpOutcome::err(ErrorCode::UnknownOp).response);
        }
        if serde_json::to_vec(&frame).map_or(true, |b| b.len() > MAX_REQUEST) {
            return Ok(OpOutcome::err(ErrorCode::InvalidInput).response);
        }
        let out = if op == "get_state" { lock_core(&self.0.core).state_answer() } else { dispatch(&self.0.core, &frame, &self.0.deps).response };
        self.0.shared.flush();
        Ok(out)
    }
}

struct EngineTransport<'a> {
    inner: &'a Inner,
    origin: String,
    /// The provider answered at least one request with a 2xx: only then
    /// does a flow count as provider contact (§11.3.2; review SEC-I2).
    answered: AtomicBool,
}

impl Transport for EngineTransport<'_> {
    fn send(&self, origin: &str, method: &str, path: &str, auth: Option<&str>, body: &[u8]) -> Result<HttpResponse, TransportError> {
        if self.inner.cancel.load(Ordering::SeqCst) {
            return Err(TransportError::Unreachable("cancelled".into()));
        }
        if origin != self.origin {
            return Err(TransportError::Unreachable("not this vault's provider".into()));
        }
        match self.inner.shared.http(origin, method, path, auth, body, TIMEOUT_MS, MAX_ANSWER) {
            HttpOutcome::Answered { status, body, date } => {
                if (200..300).contains(&status) {
                    self.answered.store(true, Ordering::SeqCst);
                }
                Ok(HttpResponse { status, body, date })
            }
            HttpOutcome::TooLarge => Err(TransportError::Unreachable("answer over the cap".into())),
            HttpOutcome::Unreachable => Err(TransportError::Unreachable("unreachable".into())),
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A provider's error code reaches the app's copy only in the §15 shape;
/// anything else is reported by its status (review SEC-I4).
fn code_ok(c: &str) -> bool {
    (1..=48).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

fn failure(f: &Failure) -> Value {
    let (code, status) = match f {
        Failure::Helper(c) => (c.clone(), None),
        Failure::Provider(s, c) if code_ok(c) => (c.clone(), Some(*s)),
        Failure::Provider(s, _) => (format!("PROVIDER_{s}"), Some(*s)),
        Failure::Unreachable => ("BACKUP_UNAVAILABLE".into(), None),
        Failure::Conflict => ("BACKUP_CONFLICT".into(), None),
    };
    json!({ "ok": false, "error": code, "status": status })
}

/// `provider_sync {interactive}` / `provider_publish {interactive}`, and
/// total-loss recovery onto this phone: `provider_recover_start {handle,
/// kind}` (UNINITIALIZED → the FR-01 preview) and
/// `provider_recover_finish` (RECOVERING → UNLOCKED on a fresh key).
pub fn run(inner: &Inner, frame: &Value) -> OpOutcome {
    let op = frame.get("op").and_then(Value::as_str).unwrap_or("");
    let interactive = frame.get("interactive").and_then(Value::as_bool) == Some(true);
    if !matches!(op, "provider_sync" | "provider_publish" | "provider_recover_start" | "provider_recover_finish") {
        return OpOutcome::err(ErrorCode::UnknownOp);
    }
    let recover = op.starts_with("provider_recover");
    // Sync and publish: UNLOCKED, except that a fully staged publication
    // also finishes LOCKED (below); the vault header names the origin.
    // Recovery: there is no vault yet, so the origin is the engine's own
    // default, checked against the allowlist (§11.5).
    let origin = {
        let c = lock_core(&inner.core);
        let wanted = match op {
            "provider_recover_start" => VaultState::Uninitialized,
            "provider_recover_finish" => VaultState::Recovering,
            _ => VaultState::Unlocked,
        };
        // §22.10 / §22.11: a fully staged publication (a security change
        // cut short by a lock) finishes while the vault is LOCKED — at the
        // next foreground — with no key: only what is staged is signed.
        let resume = op == "provider_publish" && c.state == VaultState::Locked && c.provider.publish.is_some();
        // `{resume: true}` (the foreground trigger) only ever finishes such
        // an upload, never stages a new one — even if an unlock raced it
        // (chunk-3 review SEC-O1).
        let resume_only = frame.get("resume").and_then(Value::as_bool) == Some(true);
        if (c.state != wanted && !resume) || (resume_only && !resume) {
            return OpOutcome::err(ErrorCode::BadState);
        }
        match (recover, c.header.as_ref()) {
            (true, _) => match default_provider() {
                Ok(o) => o.to_string(),
                Err(e) => return OpOutcome::err(e),
            },
            (false, Some(h)) => h.provider.clone(),
            (false, None) => return OpOutcome::err(ErrorCode::BadState),
        }
    };
    // The cancel flag is never cleared: only `ov0_engine_close` sets it,
    // and a closing handle runs no further flow (review VER-O5).
    IN_PROVIDER.with(|f| f.set(true));
    let _guard = Guard;
    let transport = EngineTransport { inner, origin: origin.clone(), answered: AtomicBool::new(false) };
    let flows = Flows { helper: &EngineHelper(inner), transport: &transport };
    let sync = |f: &Flows<'_>| if interactive { f.run_sync_by_user() } else { f.run_sync() };
    // A removed phone authors nothing (§22.9), so its publish is the sync
    // — the provider-confirmed state that may lift the lock (chunk-2
    // review SEC-B2).
    let removed = {
        let c = lock_core(&inner.core);
        c.phone && removal::active(true, &c.vault_dir).is_some()
    };
    let result = if op == "provider_recover_start" {
        let (handle, kind) = (frame.get("handle").and_then(Value::as_str).unwrap_or(""), frame.get("kind").and_then(Value::as_str).unwrap_or(""));
        flows.recovery_start(&origin, handle, kind)
    } else if op == "provider_recover_finish" {
        flows.recovery_finish()
    } else if op == "provider_sync" || removed {
        sync(&flows)
    } else {
        // "Sync now" always checks the provider (§22.7): with nothing of
        // its own to publish, it syncs, as the Mac's "Back up now" does
        // (reviews VER-B1 / SEC-I2).
        let published = if interactive { flows.backup_now_by_user() } else { flows.backup_now() };
        match published {
            Ok(v) if v["nothing_to_publish"] == true && v["behind"] != true => sync(&flows),
            other => other,
        }
    };
    if !recover {
        settle(inner, &flows, &origin, &result, transport.answered.load(Ordering::SeqCst));
    }
    OpOutcome { response: result.map_or_else(|f| failure(&f), |v| json!({ "ok": true, "result": v })) }
}

/// After a flow: the access-loss count (§11.3.2), and the §22.9 removal
/// lock — set by a provider state that revokes this phone, lifted by one
/// in which it is still active.
fn settle(inner: &Inner, flows: &Flows<'_>, origin: &str, result: &Result<Value, Failure>, answered: bool) {
    // The locate probe runs before the core is locked: no network wait
    // ever holds the engine (lock and `get_state` stay immediate).
    let probe = matches!(result, Err(Failure::Provider(401, _))).then(|| {
        let body = format!(r#"{{"handle_key":"{}"}}"#, "0".repeat(64));
        flows.transport.send(origin, "POST", "/v2/recover/locate", None, body.as_bytes()).map_or((false, None), |r| (r.status == 200, r.date))
    });
    let c = lock_core(&inner.core);
    let (dir, phone) = (c.vault_dir.clone(), c.phone);
    // A revoking state locks the engine and closes the store first: the
    // §22.9 lock is recorded before anything needs the store (review
    // SEC-I1 / VER-I1).
    let revoked = result.as_ref().is_ok_and(|v| v["revoked"] == true || v["sync"]["revoked"] == true);
    if phone && revoked {
        let _ = removal::record(&dir, true, now());
    }
    let Some(store) = c.store.as_ref() else { return };
    let saved: Option<(u32, Option<u64>)> = kv::get(&store.conn, WATCH_KEY).ok().flatten();
    let mut watch = saved.map_or_else(AccessWatch::default, |(s, t)| AccessWatch::from_parts(s, t));
    match result {
        // Success with no provider answer (nothing to publish while
        // behind) is no provider contact: nothing resets, nothing lifts.
        Ok(_) if !answered => return,
        Ok(_) => {
            watch.success();
            let _ = kv::delete(&store.conn, ACCESS_LOST_KEY);
            if phone && !revoked && removal::active(true, &dir).is_some() {
                let _ = removal::clear(&dir); // provider-confirmed: still active (§22.9)
            }
        }
        Err(_) => {
            if let Some((locate_ok, date)) = probe {
                if watch.unauthorized(now(), date, locate_ok) == AccessSignal::AccessLost {
                    let _ = kv::put(&store.conn, ACCESS_LOST_KEY, &true);
                }
            }
        }
    }
    let _ = kv::put(&store.conn, WATCH_KEY, &watch.parts());
}
