//! Unlocking with this device's own envelope (§2.8).
//!
//! The master-password path (§1.5 `begin_recovery_unlock`) exists for
//! recovery and for a device that has no envelope. The ordinary path is
//! this one: a local user-presence check, then the Secure Enclave
//! decapsulates `wraps/devices/<self>.wrap` and hands back the VK and
//! this device's `device_backup_cred` in one step. No master password,
//! and the agreement private key never leaves the Enclave.
//!
//! What this refuses, and why each refusal matters:
//!
//! - **No SE key** — the device was restored from a backup, or the key
//!   was wiped. §2.8 calls this documented behavior, not corruption: the
//!   device must re-enroll or recover, so the error names that.
//! - **Not enrolled, or revoked** — a revoked device may still have an
//!   envelope on disk; the registry is what decides, and it says no.
//! - **Stale generation** — an envelope from before a VK rotation holds
//!   a dead key. Opening the vault with it would either fail deeper in
//!   or, worse, appear to work against stale files.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::setup::{install_unlock, unlock_failed_nonfatal};
use super::{ev_state, lock_core, Deps, OpOutcome, VaultCore};
use crate::crypto::secret::SecretBytes;
use crate::crypto::wrap::RecoveryWrapPayload;
use crate::device::envelope;
use crate::device::identity::SeDevice;
use crate::errors::ErrorCode;
use crate::registry::chain::EpochPolicy;
use crate::registry::device::DeviceIdentity;
use crate::registry::log;
use crate::state::VaultState;

const POLICY: EpochPolicy<'static> = EpochPolicy::CheckpointAnchored;

/// §1.5 `unlock`: presence → envelope → VK. No secret crosses IPC in
/// either direction; the response is a state, like every other unlock.
pub fn unlock(core: &Arc<Mutex<VaultCore>>, deps: &Deps) -> OpOutcome {
    let (header, dir) = {
        let mut c = lock_core(core);
        if c.state != VaultState::Locked {
            return OpOutcome::err(ErrorCode::BadState);
        }
        if let Some(code) = c.header_error {
            c.enter_error(&deps.events);
            return OpOutcome::err(code);
        }
        let Some(header) = c.header.clone() else {
            return OpOutcome::err(ErrorCode::DbCorrupt);
        };
        c.state = VaultState::Unlocking;
        (header, c.vault_dir.clone())
    };
    deps.events.emit(ev_state(VaultState::Unlocking));

    // §6.4: one LA evaluation, the same policy as every other presence
    // gate. A refusal is not a failed credential — nothing is counted
    // against the backoff.
    if !deps.la.check("Source Vault: unlock") {
        return unlock_failed_nonfatal(core, ErrorCode::PresenceDenied, deps);
    }

    let me = match SeDevice::load(&dir) {
        Ok(d) => d,
        // §2.8: a missing SE key means re-enroll or recover. The MP path
        // is still open to this user, which is what makes that recovery.
        Err(_) => return unlock_failed_nonfatal(core, ErrorCode::DeviceNotAuthorized, deps),
    };

    // The registry decides whether this device may still open the vault.
    // Its own envelope on disk is not the authority.
    match log::read_state(&dir, &header.vault_id.0, &POLICY) {
        Ok(state) if state.active_device(&me.device_id()).is_some() => {}
        Ok(_) => return unlock_failed_nonfatal(core, ErrorCode::DeviceNotAuthorized, deps),
        Err(e) => return unlock_failed_nonfatal(core, e, deps),
    }

    let file = match envelope::read_envelope(&dir, &me.device_id()) {
        Ok(f) => f,
        Err(_) => return unlock_failed_nonfatal(core, ErrorCode::DeviceNotAuthorized, deps),
    };
    let payload = match envelope::open_envelope(me.key_tag(), &header.vault_id.0, &file) {
        Ok(p) => p,
        Err(e) => return unlock_failed_nonfatal(core, e, deps),
    };
    // An envelope from before a rotation carries a retired VK (§2.10).
    if payload.vk_generation != header.vk_generation {
        return unlock_failed_nonfatal(core, ErrorCode::WrapCorrupt, deps);
    }

    // `device_backup_cred` travels with the VK (§2.8) and stays in the
    // helper; nothing about it crosses IPC.
    let recovered = RecoveryWrapPayload {
        vk: SecretBytes::new(*payload.vk.expose()),
        wrapped_at: payload.wrapped_at,
        vk_generation: payload.vk_generation,
    };
    drop(payload);
    let outcome = install_unlock(core, &header, &dir, recovered, deps);
    if outcome.response["ok"] == Value::Bool(true) {
        return OpOutcome::ok(json!({"state": "unlocked", "method": "device"}));
    }
    outcome
}
