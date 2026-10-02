//! Helper preferences (§1.6): the auto-lock dial.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::{lock_core, OpOutcome, VaultCore};
use crate::errors::ErrorCode;
use crate::state::VaultState;

/// Phase C internal op (not in §1.5 — the catalog has no prefs op; the
/// §1.6 "configurable 5–60" dial needs one). Documented in the Phase C
/// verification report.
pub fn set_auto_lock_minutes(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
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
