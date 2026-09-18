//! §1.5 `change_master_password`: UNLOCKED + fresh presence; the helper
//! panel collects old + new (§1.7); VK never changes hands — only the
//! wrap is re-sealed (§2.5). RK locator/cred re-registration (§12
//! scenario 5) is Phase D scope — no provider exists yet.

use std::sync::{Arc, Mutex};

use serde_json::json;

use super::setup::{emit_panel, wrap_kdf};
use super::{
    ev_state, lock_core, Deps, OpOutcome, PanelOutcome, PanelRequest, VaultCore,
};
use crate::crypto::kdf::{self, Argon2Params};
use crate::crypto::secret;
use crate::crypto::wrap::{self, PasswordWrapFile, RecoveryWrapPayload};
use crate::errors::ErrorCode;
use crate::state::VaultState;
use crate::storage::header::{write_header, Header};
use crate::storage::store::{write_atomic, PASSWORD_WRAP_NAME};
use crate::VAULT_HEADER_NAME;

const PANEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// §1.5 `change_master_password`: UNLOCKED + fresh presence; the helper
/// panel collects old + new (§1.7); VK never changes hands — only the
/// wrap is re-sealed (§2.5). RK locator/cred re-registration (§12
/// scenario 5) is Phase D scope — no provider exists yet.
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
    // The header MUST come from the live store, not the core cache:
    // mutating ops advance manifest_generation via `persist_head`, which
    // updates only the store's copy. Rewriting a stale cached header here
    // would desynchronize header.json from manifest.json (§3.5) and the
    // next open would fail closed with MANIFEST_MISMATCH.
    let (header, vault_dir) = {
        let c = lock_core(core);
        if c.state != VaultState::Authorizing {
            return OpOutcome::err(ErrorCode::BadState);
        }
        let Some(store) = c.store.as_ref() else {
            return finish_change(core, deps, Err(ErrorCode::DbCorrupt));
        };
        (store.header.clone(), c.vault_dir.clone())
    };
    let result = rewrap_under_new_mp(&header, &vault_dir, &old_mp, &new_mp);
    drop(old_mp); // SecretVec zeroize
    drop(new_mp);
    match result {
        Ok(new_header) => {
            // Refresh BOTH copies: the core cache and the store's header
            // (a later persist_head writes the store's copy — leaving the
            // old kdf salt there would silently revert header.json).
            let mut c = lock_core(core);
            if let Some(store) = c.store.as_mut() {
                store.header = new_header.clone();
            }
            c.header = Some(new_header);
            drop(c);
            finish_change(core, deps, Ok(()))
        }
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

/// Verify the old MP against the current wrap, then seal a fresh wrap
/// (new salt) under the new MP and persist wrap + header. Returns the
/// updated header so the caller can refresh the in-memory cache.
fn rewrap_under_new_mp(
    header: &Header,
    vault_dir: &std::path::Path,
    old_mp: &[u8],
    new_mp: &[u8],
) -> Result<Header, ErrorCode> {
    let wrap_bytes =
        std::fs::read(vault_dir.join(PASSWORD_WRAP_NAME)).map_err(|_| ErrorCode::WrapCorrupt)?;
    let wrap_file: PasswordWrapFile =
        serde_json::from_slice(&wrap_bytes).map_err(|_| ErrorCode::WrapCorrupt)?;
    let (salt, params) = wrap_kdf(&wrap_file)?;
    let old_pk = kdf::derive_pk(old_mp, &salt, params).map_err(|_| ErrorCode::Internal)?;
    let payload = match wrap::open_wrap_mp(&wrap_file, &old_pk, &header.vault_id.0) {
        Ok(p) => p,
        Err(crate::crypto::CryptoError::IntegrityFailure) => {
            return Err(ErrorCode::WrongCredential)
        }
        Err(_) => return Err(ErrorCode::WrapCorrupt),
    };
    let new_salt = secret::random_salt();
    let new_pk =
        kdf::derive_pk(new_mp, &new_salt, Argon2Params::V1).map_err(|_| ErrorCode::Internal)?;
    let payload = RecoveryWrapPayload {
        vk: payload.vk,
        wrapped_at: crate::storage::store::now_epoch(),
        vk_generation: payload.vk_generation,
    };
    let new_file = wrap::seal_wrap_mp(&payload, &new_pk, &header.vault_id.0, Argon2Params::V1, &new_salt)
        .map_err(|_| ErrorCode::Internal)?;
    let wrap_json = serde_json::to_vec_pretty(&new_file).map_err(|_| ErrorCode::Internal)?;
    // Wrap first, then header: at unlock the PK derives from the wrap's
    // own parameter copy, so a crash between the two writes cannot brick
    // the vault (header's kdf block is the policy record, §2.3).
    write_atomic(&vault_dir.join(PASSWORD_WRAP_NAME), &wrap_json)?;
    let mut new_header = header.clone();
    new_header.kdf.salt = crate::crypto::hex::encode(new_salt);
    write_atomic(&vault_dir.join(VAULT_HEADER_NAME), &write_header(&new_header)?)?;
    Ok(new_header)
}
