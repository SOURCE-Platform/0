//! Shared fixture for the op-level vault tests: stub Deps (scripted panel
//! answers, stub LA presence, stub capture check, recording event sink),
//! a real on-disk vault per test, and small op helpers. Synthetic
//! credentials only.
//!
//! Keychain isolation: one process-unique service prefix, items deleted
//! before every test, and each test binary serialized on a mutex (env and
//! rollback-generation state are process-global).
//!
//! Not every binary uses every helper; silence per-binary lints.
#![allow(dead_code, unused_imports)]

pub use std::collections::VecDeque;
pub use std::path::PathBuf;
pub use std::sync::atomic::{AtomicUsize, Ordering};
pub use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
pub use std::time::Duration;

pub use serde_json::{json, Value};
pub use vault_helper::crypto::secret::SecretVec;
pub use vault_helper::keychain;
pub use vault_helper::state::VaultState;
pub use vault_helper::storage::store::{MANIFEST_NAME, PASSWORD_WRAP_NAME};
pub use vault_helper::vault::{
    dispatch, CaptureChecker, Deps, EventSink, LockReason, PanelOutcome, PanelRequest,
    PanelRunner, PresenceChecker, RecoverySheet, VaultCore,
};
pub use vault_helper::VAULT_HEADER_NAME;

pub const MP: &[u8] = b"synthetic-master-password-0001 (test fixture, not real)";
pub const MP_NEW: &[u8] = b"synthetic-master-password-0002 (rotated fixture)";
pub const PASSWORD: &str = "synthetic-login-password (test fixture, not real)";
pub const PASSWORD2: &str = "synthetic-login-password-v2 (test fixture, not real)";

// --- stubs ------------------------------------------------------------------

pub struct Panel {
    pub queue: Mutex<VecDeque<PanelOutcome>>,
    pub seen: Mutex<Vec<PanelRequest>>,
    /// Checkpoint lines of every Recovery Key window shown.
    pub sheets: Mutex<Vec<String>>,
    pub shown_rk: Mutex<Option<String>>,
    pub refuse_sheet: Mutex<bool>,
}

impl PanelRunner for Panel {
    fn run(&self, req: PanelRequest, _timeout: Duration) -> PanelOutcome {
        self.seen.lock().unwrap().push(req);
        if req == PanelRequest::RkEntry {
            // Type back the last Recovery Key shown (in-process only).
            if let Some(words) = self.shown_rk.lock().unwrap().clone() {
                return PanelOutcome::Submitted(SecretVec::new(words.into_bytes()));
            }
        }
        self.queue
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(PanelOutcome::Cancelled)
    }

    /// Acknowledge by default and remember the words (never printed).
    fn show_recovery_key(&self, sheet: &RecoverySheet, _timeout: Duration) -> PanelOutcome {
        self.sheets.lock().unwrap().push(sheet.checkpoint.clone());
        if *self.refuse_sheet.lock().unwrap() {
            return PanelOutcome::Cancelled;
        }
        *self.shown_rk.lock().unwrap() = Some(sheet.words.to_string());
        PanelOutcome::Acknowledged
    }
}

pub struct La {
    pub allow: bool,
    pub calls: AtomicUsize,
}

impl PresenceChecker for La {
    fn check(&self, _reason: &str) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.allow
    }
}

pub struct Capture {
    pub suppressed: bool,
    pub calls: AtomicUsize,
}

impl CaptureChecker for Capture {
    fn suppressed(&self, _surface: &str) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.suppressed
    }
}

pub struct Events {
    pub log: Mutex<Vec<Value>>,
}

impl EventSink for Events {
    fn emit(&self, event: Value) {
        self.log.lock().unwrap().push(event);
    }
}

// --- fixture ----------------------------------------------------------------

pub struct Fx {
    pub dir: PathBuf,
    pub core: Arc<Mutex<VaultCore>>,
    pub deps: Deps,
    pub panel: Arc<Panel>,
    pub la: Arc<La>,
    pub capture: Arc<Capture>,
    pub events: Arc<Events>,
}

pub fn serial() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    // Poison-tolerant: one failing test must not cascade PoisonErrors
    // into the rest of the file.
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub fn fx() -> Fx {
    fx_with_presence(true)
}

/// Same fixture with the LA presence check scripted to refuse.
pub fn fx_with_presence(allow: bool) -> Fx {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    static PREFIX: OnceLock<()> = OnceLock::new();
    PREFIX.get_or_init(|| {
        std::env::set_var(
            "OV0_VAULT_KEYCHAIN_PREFIX",
            format!("ov0ops-{}-", std::process::id()),
        );
    });
    // Rollback/prefs items are process-global state; start clean.
    keychain::delete_item("com.racker.zero.vault.state");
    keychain::delete_item("com.racker.zero.vault.helper-prefs");
    let dir = PathBuf::from(format!(
        "/tmp/vhops-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let panel = Arc::new(Panel {
        queue: Mutex::new(VecDeque::new()),
        seen: Mutex::new(Vec::new()),
        sheets: Mutex::new(Vec::new()),
        shown_rk: Mutex::new(None),
        refuse_sheet: Mutex::new(false),
    });
    let la = Arc::new(La {
        allow,
        calls: AtomicUsize::new(0),
    });
    let capture = Arc::new(Capture {
        suppressed: true,
        calls: AtomicUsize::new(0),
    });
    let events = Arc::new(Events {
        log: Mutex::new(Vec::new()),
    });
    Fx {
        core: Arc::new(Mutex::new(VaultCore::boot(dir.clone()))),
        deps: Deps {
            panel: panel.clone(),
            la: la.clone(),
            capture: capture.clone(),
            events: events.clone(),
        },
        panel,
        la,
        capture,
        events,
        dir,
    }
}

/// Each fixture vault owns a Secure Enclave identity (Phase E); tearing
/// the fixture down destroys those keys so test runs leave no SE state.
impl Drop for Fx {
    fn drop(&mut self) {
        vault_helper::device::identity::wipe(&self.dir);
    }
}

impl Fx {
    /// Swap the LA presence stub in place. (Rebuilding the whole fixture
    /// is not possible: `Fx` owns Secure Enclave keys and therefore has a
    /// `Drop`.)
    pub fn set_presence(&mut self, la: Arc<La>) {
        self.deps.la = la.clone();
        self.la = la;
    }

    /// Re-boot the core against the same directory (a "helper restarted"
    /// scenario), keeping the fixture's stubs and its SE identity.
    pub fn reboot(&mut self) {
        self.core = Arc::new(Mutex::new(VaultCore::boot(self.dir.clone())));
    }

    /// Swap the capture-suppression stub in place.
    pub fn set_capture(&mut self, capture: Arc<Capture>) {
        self.deps.capture = capture.clone();
        self.capture = capture;
    }

    pub fn push_panel(&self, outcome: PanelOutcome) {
        self.panel.queue.lock().unwrap().push_back(outcome);
    }

    pub fn state(&self) -> VaultState {
        self.core.lock().unwrap().state
    }

    pub fn op(&self, frame: Value) -> Value {
        dispatch(&self.core, &frame, &self.deps).response
    }

    pub fn events_named(&self, name: &str) -> Vec<Value> {
        self.events
            .log
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.get("event").and_then(Value::as_str) == Some(name))
            .cloned()
            .collect()
    }
}

pub fn submitted(mp: &[u8]) -> PanelOutcome {
    PanelOutcome::Submitted(SecretVec::new(mp.to_vec()))
}

pub fn state_of(resp: &Value) -> &str {
    resp.get("state").and_then(Value::as_str).unwrap_or("")
}

pub fn err_code(resp: &Value) -> &str {
    resp.get("error").and_then(Value::as_str).unwrap_or("")
}

pub fn setup_vault(fx: &Fx, mp: &[u8]) -> Value {
    fx.push_panel(submitted(mp));
    fx.op(json!({"op": "setup_vault"}))
}

pub fn unlock(fx: &Fx, mp: &[u8]) -> Value {
    fx.push_panel(submitted(mp));
    fx.op(json!({"op": "begin_recovery_unlock", "kind": "mp"}))
}

pub fn add_login(fx: &Fx) -> String {
    let resp = fx.op(json!({
        "op": "add_item",
        "kind": "login",
        "title": "Fixture Login",
        "username": "fixture@example.test",
        "hosts": ["example.test"],
        "password": PASSWORD,
    }));
    assert_eq!(resp["ok"], true, "add_item failed: {resp}");
    resp["ref"].as_str().unwrap().to_string()
}

pub fn setup_and_unlock(fx: &Fx) {
    assert_eq!(setup_vault(fx, MP)["ok"], true);
    assert_eq!(fx.state(), VaultState::Locked);
    assert_eq!(unlock(fx, MP)["ok"], true);
    assert_eq!(fx.state(), VaultState::Unlocked);
}
