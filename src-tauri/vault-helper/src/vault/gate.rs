//! The AUTHORIZING gate shared by every mutation/secret-release op
//! (§13.3: unlock ≠ authorization, one LA presence check per op, no grace
//! window), plus the §1.5 `reveal` op with its §14.4 capture check.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::{ev_capture_unsafe, ev_state, lock_core, Deps, OpOutcome, VaultCore};
use crate::errors::ErrorCode;
use crate::state::VaultState;
use crate::storage::records;

/// Transition UNLOCKED → AUTHORIZING and run the LA presence check
/// (§6.4: one `deviceOwnerAuthentication` call; no custom password UI).
/// On denial the state returns to UNLOCKED before the op answers.
pub(super) fn presence_gate(
    core: &Arc<Mutex<VaultCore>>,
    deps: &Deps,
    reason: &str,
) -> Result<(), OpOutcome> {
    {
        let mut c = lock_core(core);
        if c.state != VaultState::Unlocked {
            return Err(OpOutcome::err(ErrorCode::BadState));
        }
        c.state = VaultState::Authorizing;
    }
    deps.events.emit(ev_state(VaultState::Authorizing));
    if deps.la.check(reason) {
        return Ok(());
    }
    let mut c = lock_core(core);
    if c.state == VaultState::Authorizing {
        c.state = VaultState::Unlocked;
        deps.events.emit(ev_state(VaultState::Unlocked));
    }
    Err(OpOutcome::err(ErrorCode::PresenceDenied))
}

/// Close out an AUTHORIZING op: verify the state is still ours (a lock
/// may have preempted during the LA wait), run the body against the live
/// store+VK, flip back to UNLOCKED, and stamp the authorization clock
/// (§1.6 auto-lock base).
macro_rules! finish_authorized {
    ($core:expr, $deps:expr, $body:expr) => {{
        use $crate::errors::ErrorCode as Ec;
        use $crate::state::VaultState as Vs;
        use $crate::vault::{ev_state, lock_core, OpOutcome};
        let mut c = lock_core($core);
        if c.state != Vs::Authorizing {
            return OpOutcome::err(Ec::BadState);
        }
        let result: Result<Value, Ec> = {
            // Reborrow through the guard so the field borrows split.
            let c = &mut *c;
            let store = match c.store.as_mut() {
                Some(s) => s,
                None => return OpOutcome::err(Ec::Internal),
            };
            let vk = match c.vk.as_ref() {
                Some(v) => v,
                None => return OpOutcome::err(Ec::Internal),
            };
            let body: &dyn Fn(
                &mut $crate::storage::VaultStore,
                &$crate::crypto::secret::SecretBytes<32>,
            ) -> Result<Value, Ec> = &$body;
            body(store, vk)
        };
        c.state = Vs::Unlocked;
        $deps.events.emit(ev_state(Vs::Unlocked));
        match result {
            Ok(extra) => {
                c.note_authorization();
                OpOutcome::ok(extra)
            }
            Err(e) => OpOutcome::err(e),
        }
    }};
}

pub(crate) use finish_authorized;

/// §1.5 `reveal`: one-shot display of one record's secret fields.
/// §14.4 order: capture check first (refuse without prompting presence
/// when suppression is unverifiable), then presence, then decrypt.
pub fn reveal(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let Some(r) = frame.get("ref").and_then(Value::as_str) else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    {
        let c = lock_core(core);
        if c.state != VaultState::Unlocked {
            return OpOutcome::err(ErrorCode::BadState);
        }
    }
    if !deps.capture.suppressed("vault") {
        deps.events.emit(ev_capture_unsafe("reveal"));
        return OpOutcome::err(ErrorCode::CaptureUnsafe);
    }
    if let Err(o) = presence_gate(core, deps, "Source Vault: reveal one item") {
        return o;
    }
    finish_authorized!(core, deps, move |store: &mut crate::storage::VaultStore,
                                         vk: &crate::crypto::secret::SecretBytes<32>| {
        let tip = store.read_tip(vk, r)?;
        let plaintext: Value =
            serde_json::from_slice(&tip.plaintext).map_err(|_| ErrorCode::RecordCorrupt)?;
        let secret = extract_secret(tip.kind_tag, &plaintext)?;
        Ok(json!({
            "ref": r,
            "kind": records::kind_name(tip.kind_tag),
            "secret": secret,
        }))
    })
}

/// One record's secret fields (§1.5 reveal contract: never more than one
/// record, never history/notes dumps).
fn extract_secret(kind_tag: u8, plaintext: &Value) -> Result<Value, ErrorCode> {
    match kind_tag {
        records::KIND_LOGIN => {
            let password = plaintext
                .get("password")
                .and_then(Value::as_str)
                .ok_or(ErrorCode::RecordCorrupt)?;
            Ok(json!({"password": password}))
        }
        records::KIND_CARD => Ok(json!({
            "number": plaintext.get("number").cloned().unwrap_or(Value::Null),
            "expiry": plaintext.get("expiry").cloned().unwrap_or(Value::Null),
            "cardholder": plaintext.get("cardholder").cloned().unwrap_or(Value::Null),
        })),
        _ => Err(ErrorCode::RecordCorrupt),
    }
}
