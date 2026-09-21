//! Phase D IPC ops (spec §1.5, §12 scenarios 5–7): Recovery Key unlock,
//! `rotate_recovery_key`, and the forgotten-MP reset. Every secret — MP,
//! RK bytes, RK words, VK — stays in the helper: the panels collect and
//! display them, and responses carry status codes only.
//!
//! Commit rule for new Recovery Keys: the key is shown (and printable)
//! and the user must press "I've saved it" **before** anything is
//! committed, so a dismissed window can never leave a vault protected by
//! a key nobody saw.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use zeroize::Zeroizing;

use super::setup::{emit_panel, install_unlock, unlock_failed_nonfatal};
use super::{ev_panel, ev_state, lock_core, Deps, OpOutcome, PanelOutcome, PanelRequest, RecoverySheet, VaultCore, RK_SHEET_TITLE};
use crate::crypto::bip39;
use crate::crypto::hex;
use crate::crypto::secret::{random_secret, SecretBytes};
use crate::crypto::wrap::{self, RecoveryWrapFile};
use crate::errors::ErrorCode;
use crate::recovery::sheet::head_prefix;
use crate::state::VaultState;
use crate::storage::rotation::{self, MpWrap, RkWrap};
use crate::storage::store::RECOVERY_WRAP_NAME;
use crate::storage::VaultStore;

const PANEL_TIMEOUT: Duration = Duration::from_secs(120);

/// The Recovery Key window content for `rk` (words + §11.7 checkpoint).
pub fn make_sheet(
    rk: &SecretBytes<32>,
    vault_id: &[u8; 16],
    generation: u64,
    registry_head: &[u8; 32],
    reason: super::SheetReason,
) -> RecoverySheet {
    RecoverySheet {
        reason,
        words: Zeroizing::new(bip39::encode_rk(rk)),
        checkpoint: format!(
            "Vault {}  ·  generation {generation}  ·  registry {}",
            hex::encode(vault_id),
            head_prefix(registry_head)
        ),
    }
}

/// Show the sheet with the capture-suppression bracket around it.
pub fn show_sheet(deps: &Deps, sheet: &RecoverySheet) -> bool {
    deps.events.emit(ev_panel(true, Some(RK_SHEET_TITLE)));
    let outcome = deps.panel.show_recovery_key(sheet, PANEL_TIMEOUT);
    deps.events.emit(ev_panel(false, None));
    matches!(outcome, PanelOutcome::Acknowledged)
}

/// `begin_recovery_unlock {kind:"rk"}` on this Mac (LOCKED → UNLOCKED).
pub fn begin_rk_unlock(core: &Arc<Mutex<VaultCore>>, deps: &Deps) -> OpOutcome {
    let (header, dir) = {
        let mut c = lock_core(core);
        if c.state != VaultState::Locked {
            return OpOutcome::err(ErrorCode::BadState);
        }
        let Some(header) = c.header.clone() else {
            return OpOutcome::err(ErrorCode::DbCorrupt);
        };
        c.state = VaultState::Unlocking;
        (header, c.vault_dir.clone())
    };
    deps.events.emit(ev_state(VaultState::Unlocking));
    emit_panel(deps, true, PanelRequest::RkEntry);
    let outcome = deps.panel.run(PanelRequest::RkEntry, PANEL_TIMEOUT);
    emit_panel(deps, false, PanelRequest::RkEntry);
    let PanelOutcome::Submitted(words) = outcome else {
        return unlock_failed_nonfatal(core, ErrorCode::PanelCancelled, deps);
    };
    // §2.4: normalize + wordlist + checksum, offline, before any use.
    let rk = match std::str::from_utf8(&words).ok().map(bip39::decode_rk) {
        Some(Ok(rk)) => rk,
        _ => return unlock_failed_nonfatal(core, ErrorCode::RecoveryKeyInvalid, deps),
    };
    drop(words);
    let file: RecoveryWrapFile = match std::fs::read(dir.join(RECOVERY_WRAP_NAME)).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
        Some(f) => f,
        None => return unlock_failed_nonfatal(core, ErrorCode::WrongCredential, deps),
    };
    match wrap::open_wrap_rk(&file, &rk, &header.vault_id.0) {
        Ok(payload) => install_unlock(core, &header, &dir, payload, deps),
        Err(_) => unlock_failed_nonfatal(core, ErrorCode::WrongCredential, deps),
    }
}

/// Enter AUTHORIZING with fresh presence (UNLOCKED + presence, §1.5).
fn authorize(core: &Arc<Mutex<VaultCore>>, deps: &Deps, reason: &str) -> Result<(), ErrorCode> {
    {
        let mut c = lock_core(core);
        if c.state != VaultState::Unlocked {
            return Err(ErrorCode::BadState);
        }
        c.state = VaultState::Authorizing;
    }
    deps.events.emit(ev_state(VaultState::Authorizing));
    if deps.la.check(reason) {
        Ok(())
    } else {
        Err(ErrorCode::PresenceDenied)
    }
}

/// Back to UNLOCKED unless a lock preempted the op.
pub(super) fn finish_pub(
    core: &Arc<Mutex<VaultCore>>,
    deps: &Deps,
    result: Result<(), ErrorCode>,
) -> OpOutcome {
    finish(core, deps, result)
}

/// Presence gate shared with the device ops.
pub(super) fn authorize_pub(
    core: &Arc<Mutex<VaultCore>>,
    deps: &Deps,
    reason: &str,
) -> Result<(), ErrorCode> {
    authorize(core, deps, reason)
}

fn finish(core: &Arc<Mutex<VaultCore>>, deps: &Deps, result: Result<(), ErrorCode>) -> OpOutcome {
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

/// §1.5 `rotate_recovery_key`: current MP (to re-seal password.wrap) →
/// new RK shown + acknowledged → VK rotation with both wraps rebuilt.
pub fn rotate_recovery_key(core: &Arc<Mutex<VaultCore>>, deps: &Deps) -> OpOutcome {
    if let Err(e) = authorize(core, deps, "Source Vault: replace your Recovery Key") {
        return finish(core, deps, Err(e));
    }
    emit_panel(deps, true, PanelRequest::MpEntry);
    let outcome = deps.panel.run(PanelRequest::MpEntry, PANEL_TIMEOUT);
    emit_panel(deps, false, PanelRequest::MpEntry);
    let PanelOutcome::Submitted(mp) = outcome else {
        return finish(core, deps, Err(ErrorCode::PanelCancelled));
    };
    let (pk, vault_id, next_gen, head) = {
        let c = lock_core(core);
        let Some(store) = c.store.as_ref().filter(|_| c.state == VaultState::Authorizing) else {
            return OpOutcome::err(ErrorCode::BadState);
        };
        let pk = match super::recovery_ops::prove_mp(store, &mp) {
            Ok(pk) => pk,
            Err(e) => {
                drop(c);
                return finish(core, deps, Err(e));
            }
        };
        (pk, store.header.vault_id.0, store.header.manifest_generation + 1, store.header.registry_head.0)
    };
    drop(mp);
    let rk = random_secret();
    if !show_sheet(deps, &make_sheet(&rk, &vault_id, next_gen, &head, super::SheetReason::Replaced)) {
        return finish(core, deps, Err(ErrorCode::PanelCancelled)); // nothing committed
    }
    let mut c = lock_core(core);
    if c.state != VaultState::Authorizing {
        return OpOutcome::err(ErrorCode::BadState); // a lock landed meanwhile
    }
    let (Some(store), Some(vk)) = (c.store.take(), c.vk.take()) else {
        return OpOutcome::err(ErrorCode::BadState);
    };
    let dir = store.dir.clone();
    let rotated = rotation::rotate(store, &vk, MpWrap::Reseal(&pk), RkWrap::Seal(&rk), None, None);
    drop(vk);
    let reopened = rotated.and_then(|r| VaultStore::open(&dir).map(|s| (r, s)));
    match reopened {
        Ok((r, store)) => {
            c.header = Some(store.header.clone());
            let generation = store.header.manifest_generation;
            c.store = Some(store);
            c.vk = Some(r.new_vk.mlock_best_effort());
            let _ = crate::keychain::write_seen_generation(generation);
            drop(c);
            finish(core, deps, Ok(()))
        }
        Err(_) => {
            // Journal guarantees old-or-new on disk; drop to LOCKED so the
            // next unlock opens whichever state is committed.
            let events = c.lock(super::LockReason::Fatal);
            drop(c);
            for e in events {
                deps.events.emit(e);
            }
            OpOutcome::err(ErrorCode::RotationFailed)
        }
    }
}

/// `change_master_password {mode:"reset"}` — §12 scenario 5 on this Mac:
/// the MP is forgotten but the vault is unlocked (e.g. via the Recovery
/// Key); presence + a new MP re-wrap the resident VK. No rotation.
pub fn reset_master_password(core: &Arc<Mutex<VaultCore>>, deps: &Deps) -> OpOutcome {
    if let Err(e) = authorize(core, deps, "Source Vault: set a new master password") {
        return finish(core, deps, Err(e));
    }
    emit_panel(deps, true, PanelRequest::MpCreate);
    let outcome = deps.panel.run(PanelRequest::MpCreate, PANEL_TIMEOUT);
    emit_panel(deps, false, PanelRequest::MpCreate);
    let PanelOutcome::Submitted(mp) = outcome else {
        return finish(core, deps, Err(ErrorCode::PanelCancelled));
    };
    let result = {
        let mut c = lock_core(core);
        if c.state != VaultState::Authorizing {
            return OpOutcome::err(ErrorCode::BadState);
        }
        let vk = c.vk.as_ref().map(|v| SecretBytes::new(*v.expose()));
        match (c.store.as_mut(), vk) {
            (Some(store), Some(vk)) => super::recovery_ops::set_master_password(store, &vk, &mp).map(|_| store.header.clone()),
            _ => Err(ErrorCode::BadState),
        }
        .map(|h| c.header = Some(h))
    };
    drop(mp);
    finish(core, deps, result)
}
