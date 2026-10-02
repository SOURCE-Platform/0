//! §1.5 `change_master_password`: UNLOCKED + fresh presence; the helper
//! panel collects old + new (§1.7); the VK never changes hands — only the
//! wrap is re-sealed (§2.5), with new kdf and MP-class auth salts and the
//! MP-change `pending_remote` component in one journaled commit
//! (§11.3.2); the next publish carries the recovery-auth update.

use std::sync::{Arc, Mutex};

use serde_json::json;

use super::setup::emit_panel;
use super::{
    ev_state, lock_core, Deps, OpOutcome, PanelOutcome, PanelRequest, VaultCore,
};
use crate::errors::ErrorCode;
use crate::state::VaultState;

const PANEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// §1.5 `change_master_password`: presence, the panel's old + new MP,
/// then `recovery_ops::change_mp`.
pub fn change_master_password(core: &Arc<Mutex<VaultCore>>, deps: &Deps) -> OpOutcome {
    {
        let mut c = lock_core(core);
        if c.state != VaultState::Unlocked {
            return OpOutcome::err(ErrorCode::BadState);
        }
        c.state = VaultState::Authorizing;
    }
    deps.events.emit(ev_state(VaultState::Authorizing));
    if !deps.la.check("Source Vault: change master password") {
        return finish_change(core, deps, Err(ErrorCode::PresenceDenied));
    }
    emit_panel(deps, true, PanelRequest::MpChange);
    let outcome = deps.panel.run(PanelRequest::MpChange, PANEL_TIMEOUT);
    emit_panel(deps, false, PanelRequest::MpChange);
    let PanelOutcome::SubmittedChange(old_mp, new_mp) = outcome else {
        return finish_change(core, deps, Err(ErrorCode::PanelCancelled));
    };
    // The store is taken for the journaled commit and put back reopened
    // (the committed state — old or new — on any failure).
    let result = {
        let mut c = lock_core(core);
        if c.state != VaultState::Authorizing {
            return OpOutcome::err(ErrorCode::BadState);
        }
        let vk = c.vk.as_ref().map(|v| crate::crypto::secret::SecretBytes::new(*v.expose()));
        match (c.store.take(), vk) {
            (Some(store), Some(vk)) => {
                let dir = store.dir.clone();
                match super::recovery_ops::change_mp(store, &vk, Some(&old_mp), &new_mp) {
                    Ok(store) => {
                        c.header = Some(store.header.clone());
                        c.store = Some(store);
                        Ok(())
                    }
                    Err(e) => {
                        c.store = crate::storage::VaultStore::open(&dir).ok();
                        Err(e)
                    }
                }
            }
            _ => Err(ErrorCode::DbCorrupt),
        }
    };
    drop(old_mp); // SecretVec zeroize
    drop(new_mp);
    match result {
        Ok(()) => finish_change(core, deps, Ok(())),
        Err(code @ ErrorCode::WrongCredential) => {
            let delay = lock_core(core).record_failed_attempt();
            let outcome = finish_change(core, deps, Err(code));
            std::thread::sleep(delay);
            outcome
        }
        Err(code) => finish_change(core, deps, Err(code)),
    }
}

/// Close a change-MP op: AUTHORIZING → UNLOCKED (unless preempted),
/// restamping the authorization clock on success.
fn finish_change(
    core: &Arc<Mutex<VaultCore>>,
    deps: &Deps,
    result: Result<(), ErrorCode>,
) -> OpOutcome {
    let mut c = lock_core(core);
    if c.state == VaultState::Authorizing {
        c.state = VaultState::Unlocked;
        deps.events.emit(ev_state(VaultState::Unlocked));
    }
    match result {
        Ok(()) => {
            c.note_authorization();
            OpOutcome::ok(json!({}))
        }
        Err(e) => OpOutcome::err(e),
    }
}
