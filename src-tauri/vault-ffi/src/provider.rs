//! The phone's provider path (spec v0.5 §11.1, F.2d step 2; plan
//! `phase-f2d-plan.md` revision 2): `vault-coordinator`'s flows run inside
//! one `ov0_engine_call` (`provider_sync`, `provider_publish`), with
//!
//! - `Helper` = the engine's own dispatcher, limited to `SUB_OPS`, each
//!   frame held to `MAX_REQUEST`, events flushed between sub-ops — and the
//!   op lane is never taken again (it is held by this very call);
//! - `Transport` = Swift's `http_send`, only to the vault header's own
//!   provider origin, each answer capped at `MAX_ANSWER`, 30 s each,
//!   refused once `ov0_engine_close` has set the cancel flag.
//!
//! A callback that calls back into the engine meanwhile gets `BAD_STATE`
//! (`in_provider`). The coordinator holds no secret (§11.1); everything it
//! moves is ciphertext, public data or a request the engine signed.
//! The §11.3.2 access-loss count is kept in the engine's store, since the
//! app's lifetimes are too short for a count held in memory (SPEC-I3).

use std::cell::Cell;
use std::sync::atomic::Ordering;

use serde_json::{json, Value};
use vault_coordinator::flows::Flows;
use vault_coordinator::policy::{AccessSignal, AccessWatch};
use vault_coordinator::{Failure, Helper, HttpResponse, Transport, TransportError};
use vault_engine::errors::ErrorCode;
use vault_engine::peer::client::removal;
use vault_engine::storage::kv;
use vault_engine::vault::{dispatch, lock_core, OpOutcome};

use crate::callbacks::HttpOutcome;
use crate::engine::{Inner, MAX_REQUEST};

/// The engine ops a provider flow may run (a subset of what the Mac's
/// coordinator uses; recovery stays out until F.2d step 5's design).
pub const SUB_OPS: &[&str] = &[
    "get_state", "backup_prepare", "backup_blob_list", "backup_transition_body", "backup_commit_result", "backup_state_offer",
    "backup_apply", "stream_begin", "stream_write", "stream_end", "stream_cancel", "stream_read", "sign_provider_request",
    "session_close", "remote_update_status",
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
            HttpOutcome::Answered { status, body, date } => Ok(HttpResponse { status, body, date }),
            HttpOutcome::TooLarge => Err(TransportError::Unreachable("answer over the cap".into())),
            HttpOutcome::Unreachable => Err(TransportError::Unreachable("unreachable".into())),
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn failure(f: &Failure) -> Value {
    let (code, status) = match f {
        Failure::Helper(c) => (c.clone(), None),
        Failure::Provider(s, c) if !c.is_empty() => (c.clone(), Some(*s)),
        Failure::Provider(s, _) => (format!("PROVIDER_{s}"), Some(*s)),
        Failure::Unreachable => ("BACKUP_UNAVAILABLE".into(), None),
        Failure::Conflict => ("BACKUP_CONFLICT".into(), None),
    };
    json!({ "ok": false, "error": code, "status": status })
}

/// `provider_sync {interactive}` / `provider_publish {interactive}`.
pub fn run(inner: &Inner, frame: &Value) -> OpOutcome {
    let op = frame.get("op").and_then(Value::as_str).unwrap_or("");
    let interactive = frame.get("interactive").and_then(Value::as_bool) == Some(true);
    let origin = match lock_core(&inner.core).header.as_ref() {
        Some(h) => h.provider.clone(),
        None => return OpOutcome::err(ErrorCode::BadState),
    };
    inner.cancel.store(false, Ordering::SeqCst);
    IN_PROVIDER.with(|f| f.set(true));
    let _guard = Guard;
    let transport = EngineTransport { inner, origin: origin.clone() };
    let flows = Flows { helper: &EngineHelper(inner), transport: &transport };
    let result = match (op, interactive) {
        ("provider_sync", true) => flows.run_sync_by_user(),
        ("provider_sync", false) => flows.run_sync(),
        ("provider_publish", true) => flows.backup_now_by_user(),
        ("provider_publish", false) => flows.backup_now(),
        _ => return OpOutcome::err(ErrorCode::UnknownOp),
    };
    settle(inner, &flows, &origin, &result);
    OpOutcome { response: result.map_or_else(|f| failure(&f), |v| json!({ "ok": true, "result": v })) }
}

/// After a flow: the access-loss count (§11.3.2), and the §22.9 removal
/// lock — set by a provider state that revokes this phone, lifted by one
/// in which it is still active.
fn settle(inner: &Inner, flows: &Flows<'_>, origin: &str, result: &Result<Value, Failure>) {
    // The locate probe runs before the core is locked: no network wait
    // ever holds the engine (lock and `get_state` stay immediate).
    let probe = matches!(result, Err(Failure::Provider(401, _))).then(|| {
        let body = format!(r#"{{"handle_key":"{}"}}"#, "0".repeat(64));
        flows.transport.send(origin, "POST", "/v2/recover/locate", None, body.as_bytes()).map_or((false, None), |r| (r.status == 200, r.date))
    });
    let c = lock_core(&inner.core);
    let (dir, phone) = (c.vault_dir.clone(), c.phone);
    let Some(store) = c.store.as_ref() else { return };
    let saved: Option<(u32, Option<u64>)> = kv::get(&store.conn, WATCH_KEY).ok().flatten();
    let mut watch = saved.map_or_else(AccessWatch::default, |(s, t)| AccessWatch::from_parts(s, t));
    match result {
        Ok(v) => {
            watch.success();
            let _ = kv::delete(&store.conn, ACCESS_LOST_KEY);
            let revoked = v["revoked"] == true || v["sync"]["revoked"] == true;
            if phone && revoked {
                let _ = removal::record(&dir, true, now());
            } else if phone && removal::active(true, &dir).is_some() {
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
