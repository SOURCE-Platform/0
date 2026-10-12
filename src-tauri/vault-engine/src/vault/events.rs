//! The §1.5 events the vault emits (state, lock, secure panel, capture).

use serde_json::{json, Value};

use super::LockReason;
use crate::state::VaultState;

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
