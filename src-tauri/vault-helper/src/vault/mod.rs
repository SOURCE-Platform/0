//! Vault runtime core (Phase C): the state-machine owner. Holds VK (only
//! when UNLOCKED/AUTHORIZING/UNLOCKING per §13.2), the open store, the
//! parsed header, wrong-credential backoff, and the auto-lock clock.
//!
//! Lock discipline: op handlers never hold the core mutex across a
//! blocking wait (panel, LA, capture query). The pattern per op is
//! lock → check/transition → unlock → await → re-lock → verify the state
//! is still the one this op established → crypto (fast) → respond.
//! That discipline is what lets `lock`, the auto-lock tick, and sleep/
//! screen-lock notifications zeroize VK under an in-flight op's feet;
//! the op then fails closed at its re-lock checkpoint.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::state::VaultState;
use crate::storage::header::Header;
use crate::storage::VaultStore;

pub mod change_mp;
pub mod create;
pub mod devices;
pub mod enroll_commit;
pub mod enroll_ops;
pub mod gate;
pub mod items;
pub mod recovery_ops;
pub mod registry_status;
pub mod rk_ops;
pub mod secure_ui;
pub mod setup;

pub use secure_ui::{
    PanelOutcome, PanelRequest, PanelRunner, RecoverySheet, SheetReason, RK_SHEET_TITLE,
};

/// One LA `deviceOwnerAuthentication` evaluation (§6.4 step 2 — the same
/// single policy for Touch-ID and clamshell Macs; never biometrics-only).
pub trait PresenceChecker: Send + Sync {
    fn check(&self, reason: &str) -> bool;
}

/// §14.4: is the Source capture-suppression machinery verifiably active
/// for the requesting surface? Production impl is a 500 ms reverse query
/// to the main app; any failure/timeout answers `false`.
pub trait CaptureChecker: Send + Sync {
    fn suppressed(&self, surface: &str) -> bool;
}

/// Live event sink (§1.5 helper→main events). Events must reach the main
/// app the moment they happen — `secure_panel_visible` gates Source's
/// capture suppression for the exact panel lifetime, so buffering them
/// until the op answers is not acceptable.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: Value);
}

pub struct Deps {
    pub panel: Arc<dyn PanelRunner>,
    pub la: Arc<dyn PresenceChecker>,
    pub capture: Arc<dyn CaptureChecker>,
    pub events: Arc<dyn EventSink>,
}

pub struct OpOutcome {
    pub response: Value,
}

impl OpOutcome {
    pub fn ok(extra: Value) -> OpOutcome {
        let mut response = json!({"ok": true, "error": null});
        if let (Some(dst), Some(src)) = (response.as_object_mut(), extra.as_object()) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
        OpOutcome { response }
    }

    pub fn err(code: ErrorCode) -> OpOutcome {
        OpOutcome {
            response: code.frame(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockReason {
    Explicit,
    Timeout,
    Sleep,
    ScreenLock,
    PanelTimeout,
    Fatal,
}

impl LockReason {
    pub fn as_str(self) -> &'static str {
        match self {
            LockReason::Explicit => "explicit",
            LockReason::Timeout => "timeout",
            LockReason::Sleep => "sleep",
            LockReason::ScreenLock => "screen_lock",
            LockReason::PanelTimeout => "panel_timeout",
            LockReason::Fatal => "fatal",
        }
    }
}

pub fn ev_state(state: VaultState) -> Value {
    json!({"event": "state", "state": state.as_str()})
}

pub fn ev_locked(reason: LockReason) -> Value {
    json!({"event": "locked", "reason": reason.as_str()})
}

/// §1.5 event. `title` is present when `visible` so the main app can
/// register the exact panel window title (§14.2).
pub fn ev_panel(visible: bool, title: Option<&str>) -> Value {
    json!({"event": "secure_panel_visible", "visible": visible, "title": title})
}

pub fn ev_capture_unsafe(op: &str) -> Value {
    json!({"event": "capture_unsafe", "op": op})
}

pub struct VaultCore {
    pub state: VaultState,
    pub vk: Option<SecretBytes<32>>,
    pub store: Option<VaultStore>,
    pub header: Option<Header>,
    pub header_error: Option<ErrorCode>,
    pub failed_attempts: u32,
    pub last_authorization: Option<Instant>,
    pub auto_lock_minutes: u32,
    pub vault_dir: PathBuf,
    /// The one in-flight device enrollment, if any (§5: one session at
    /// a time, torn down on lock, cancel, expiry or failure).
    pub enroll: Option<crate::enroll::EnrollSession>,
}

impl VaultCore {
    /// Boot: detect state, parse+ cache the header when a vault exists
    /// (parse failure is cached and surfaced on the unlock path), load
    /// the auto-lock pref (§1.6, Keychain helper-prefs).
    pub fn boot(vault_dir: PathBuf) -> VaultCore {
        let state = crate::state::detect_boot_state(&vault_dir);
        let (header, header_error) = if state == VaultState::Locked {
            match VaultStore::read_header(&vault_dir) {
                Ok(h) => (Some(h), None),
                Err(e) => (None, Some(e)),
            }
        } else {
            (None, None)
        };
        VaultCore {
            state,
            vk: None,
            store: None,
            header,
            header_error,
            failed_attempts: 0,
            last_authorization: None,
            auto_lock_minutes: crate::keychain::read_auto_lock_minutes(),
            vault_dir,
            enroll: None,
        }
    }

    /// Zeroize key material and drop the store on entry to
    /// Locked/Error (§13.3). Returns the events to emit.
    pub fn lock(&mut self, reason: LockReason) -> Vec<Value> {
        let had_vault_state = self.state != VaultState::Uninitialized;
        self.vk = None; // SecretBytes zeroizes on drop (and munlocks)
        self.store = None;
        // An enrollment in flight does not survive a lock: its secret is
        // zeroized and the phone must rescan (§5.3).
        self.enroll = None;
        self.last_authorization = None;
        let mut events = Vec::new();
        if had_vault_state {
            self.state = VaultState::Locked;
            events.push(ev_locked(reason));
            events.push(ev_state(VaultState::Locked));
        }
        events
    }

    /// Fatal vault-data path (§3.6): zeroize, ERROR state, emit.
    pub fn enter_error(&mut self, events: &Arc<dyn EventSink>) {
        self.vk = None;
        self.store = None;
        self.last_authorization = None;
        self.state = VaultState::Error;
        events.emit(ev_state(VaultState::Error));
    }

    /// §15 backoff: 500 ms × 2^(attempts-1), capped at 30 s.
    pub fn record_failed_attempt(&mut self) -> Duration {
        self.failed_attempts = self.failed_attempts.saturating_add(1);
        let shift = (self.failed_attempts - 1).min(7);
        Duration::from_millis((500u64 << shift).min(30_000))
    }

    pub fn note_authorization(&mut self) {
        self.last_authorization = Some(Instant::now());
    }

    /// §1.6 auto-lock: 15 min (configurable 5–60) since last authorization.
    pub fn auto_lock_due(&self) -> bool {
        if !self.state.vk_resident() || self.state == VaultState::Unlocking {
            return false;
        }
        let Some(last) = self.last_authorization else {
            return false;
        };
        last.elapsed() >= self.auto_lock_window()
    }

    /// Debug builds honor `OV0_VAULT_AUTO_LOCK_SECS` (seconds granularity)
    /// so the gate can observe auto-lock without waiting minutes; the
    /// override is compiled out of release builds entirely.
    fn auto_lock_window(&self) -> Duration {
        #[cfg(debug_assertions)]
        if let Ok(secs) = std::env::var("OV0_VAULT_AUTO_LOCK_SECS") {
            if let Ok(secs) = secs.parse::<u64>() {
                return Duration::from_secs(secs);
            }
        }
        Duration::from_secs(self.auto_lock_minutes as u64 * 60)
    }
}

pub fn lock_core(core: &Arc<Mutex<VaultCore>>) -> MutexGuard<'_, VaultCore> {
    core.lock().unwrap_or_else(|e| e.into_inner())
}

/// Dispatch one post-hello op (§1.5 Phase C subset). `get_state`, `lock`,
/// and `hello` are handled by the server layer (they need no vault deps).
pub fn dispatch(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let op = frame.get("op").and_then(Value::as_str).unwrap_or("");
    match op {
        "setup_vault" => setup::setup_vault(core, deps),
        "begin_recovery_unlock" => setup::begin_recovery_unlock(core, frame, deps),
        "change_master_password" if frame.get("mode").and_then(Value::as_str) == Some("reset") => {
            rk_ops::reset_master_password(core, deps)
        }
        "change_master_password" => change_mp::change_master_password(core, deps),
        "rotate_recovery_key" => rk_ops::rotate_recovery_key(core, deps),
        "list_items" => items::list_items(core),
        "add_item" => items::add_item(core, frame, deps),
        "update_item" => items::update_item(core, frame, deps),
        "delete_item" => items::delete_item(core, frame, deps),
        "reveal" => gate::reveal(core, frame, deps),
        "begin_enrollment" => enroll_ops::begin_enrollment(core, frame),
        "enroll_hello" => enroll_ops::enroll_hello(core, frame),
        "enroll_confirm" => enroll_ops::enroll_confirm(core, deps),
        "enroll_ack" => enroll_commit::enroll_ack(core, frame),
        "cancel_enrollment" => enroll_ops::cancel_enrollment(core),
        "list_devices" => devices::list_devices(core),
        "registry_status" => registry_status::registry_status(core),
        "revoke_device" => devices::revoke_device(core, frame, deps),
        "set_auto_lock_minutes" => set_auto_lock_minutes(core, frame),
        _ => OpOutcome::err(ErrorCode::UnknownOp),
    }
}

/// Phase C internal op (not in §1.5 — the catalog has no prefs op; the
/// §1.6 "configurable 5–60" dial needs one). Documented in the Phase C
/// verification report.
fn set_auto_lock_minutes(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    let minutes = frame.get("minutes").and_then(Value::as_u64);
    let Some(minutes) = minutes else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    let mut core = lock_core(core);
    if core.state != VaultState::Unlocked {
        return OpOutcome::err(ErrorCode::BadState);
    }
    match crate::keychain::write_auto_lock_minutes(minutes as u32) {
        Ok(()) => {
            core.auto_lock_minutes = minutes as u32;
            OpOutcome::ok(json!({}))
        }
        Err(e) => OpOutcome::err(e),
    }
}
