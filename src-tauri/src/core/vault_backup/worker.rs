//! The backup worker thread. Triggers (setup, unlock, an item change,
//! "back up now", network change) run a cycle at once; otherwise it runs
//! every 15 minutes, or on the §11.3.2 backoff after a failure.

use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use vault_coordinator::flows::Flows;
use vault_coordinator::policy::{self, AccessSignal, AccessWatch};
use vault_coordinator::Failure;

use super::transport::ProviderHttp;
use super::AppHelper;

pub const BACKUP_EVENT_CHANNEL: &str = "vault:backup";
const PERIOD: Duration = Duration::from_secs(15 * 60);

pub enum Trigger {
    /// Run a cycle now.
    Now,
    /// Post a publication the helper already staged (setup's `create`).
    Staged(Value),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BackupStatus {
    /// "idle" | "working" | "ok" | "offline" | "error" | "conflict" |
    /// "access_lost" | "clock_skew" | "not_configured"
    pub state: String,
    pub last_success: Option<u64>,
    pub last_error: Option<String>,
    pub attempts: u32,
    pub stale: bool,
}

struct Worker {
    tx: Sender<Trigger>,
    status: Mutex<BackupStatus>,
}

static WORKER: OnceLock<Worker> = OnceLock::new();

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Start the worker once (app setup).
pub fn start(app: AppHandle) {
    let (tx, rx) = channel::<Trigger>();
    if WORKER.set(Worker { tx, status: Mutex::new(BackupStatus { state: "idle".into(), ..Default::default() }) }).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("vault-backup".into())
        .spawn(move || {
            let mut watch = AccessWatch::default();
            let mut delay = PERIOD;
            loop {
                let trigger = match rx.recv_timeout(delay) {
                    Ok(t) => t,
                    Err(RecvTimeoutError::Timeout) => Trigger::Now,
                    Err(RecvTimeoutError::Disconnected) => return,
                };
                delay = cycle(&app, trigger, &mut watch);
            }
        })
        .ok();
}

pub fn trigger(t: Trigger) {
    if let Some(w) = WORKER.get() {
        let _ = w.tx.send(t);
    }
}

pub fn status() -> BackupStatus {
    WORKER.get().map(|w| w.status.lock().unwrap_or_else(|e| e.into_inner()).clone()).unwrap_or_default()
}

fn set_status(app: &AppHandle, f: impl FnOnce(&mut BackupStatus)) {
    if let Some(w) = WORKER.get() {
        let mut s = w.status.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut s);
        s.stale = policy::stale(s.last_success, now());
        let _ = app.emit(BACKUP_EVENT_CHANNEL, json!(*s));
    }
}

/// One cycle; returns the delay until the next scheduled one.
fn cycle(app: &AppHandle, trigger: Trigger, watch: &mut AccessWatch) -> Duration {
    let Some(origin) = super::origin() else {
        set_status(app, |s| s.state = "not_configured".into());
        return PERIOD;
    };
    set_status(app, |s| s.state = "working".into());
    let transport = ProviderHttp;
    let flows = Flows { helper: &AppHelper, transport: &transport };
    let result = match trigger {
        Trigger::Staged(p) => flows.run_publication(&p).map(|_| ()),
        Trigger::Now => run_unlocked(&flows),
    };
    match result {
        Ok(()) => {
            watch.success();
            set_status(app, |s| {
                *s = BackupStatus { state: "ok".into(), last_success: Some(now()), ..Default::default() };
            });
            PERIOD
        }
        Err(f) => {
            let mut attempts = 0;
            set_status(app, |s| {
                s.attempts += 1;
                attempts = s.attempts;
                let (state, err) = classify(&flows, origin, &f, watch);
                s.state = state.into();
                s.last_error = Some(err);
            });
            Duration::from_secs(policy::backoff(attempts))
        }
    }
}

/// Sync, then publish if anything changed. Nothing to do while locked
/// (a staged publication arrives as `Trigger::Staged`).
fn run_unlocked(flows: &Flows<'_>) -> Result<(), Failure> {
    let state = vault_coordinator::Helper::op(&AppHelper, json!({"op": "get_state"})).map_err(Failure::Helper)?;
    if state["state"] != "unlocked" {
        return Ok(());
    }
    flows.run_sync()?;
    flows.backup_now().map(|_| ())
}

fn classify(flows: &Flows<'_>, origin: &str, f: &Failure, watch: &mut AccessWatch) -> (&'static str, String) {
    match f {
        Failure::Unreachable => ("offline", "BACKUP_UNAVAILABLE".into()),
        Failure::Conflict => ("conflict", "BACKUP_CONFLICT".into()),
        Failure::Provider(401, _) => {
            // Is the provider otherwise reachable (locate answers any
            // handle), and does our clock agree with its `Date`?
            let probe = flows.transport.send(origin, "POST", "/v2/recover/locate", None, br#"{"handle_key":"0000000000000000000000000000000000000000000000000000000000000000"}"#);
            let (locate_ok, date) = match probe {
                Ok(r) => (r.status == 200, r.date),
                Err(_) => (false, None),
            };
            match watch.unauthorized(now(), date, locate_ok) {
                AccessSignal::AccessLost => ("access_lost", "BACKUP_ACCESS_LOST".into()),
                AccessSignal::ClockSkew => ("clock_skew", "CLOCK_SKEW".into()),
                _ => ("error", "AUTH_INVALID".into()),
            }
        }
        Failure::Provider(_, code) => ("error", code.clone()),
        Failure::Helper(code) => ("error", code.clone()),
    }
}
