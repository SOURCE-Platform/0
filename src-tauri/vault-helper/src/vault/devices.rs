//! Device listing and revocation (§4, §11.4, §12 scenario 8).
//!
//! Revocation is two things that must both happen: the registry `revoke`
//! entry (so no device accepts anything that device signs from now on)
//! and a VK rotation (so a copy of the old VK the device kept cannot
//! decrypt anything written afterwards). The rotation is not optional
//! and is not a separate user action — §12 scenario 8 makes it part of
//! revoking.
//!
//! Because rotation rewrites `recovery.wrap` under the new VK and the
//! helper never retains the Recovery Key, revocation necessarily issues
//! a **new** Recovery Key, shown and acknowledged in the helper's own
//! window before anything is committed. (Spec clarification, Phase E:
//! §2.10 requires every wrap to be rewritten but does not say where the
//! RK for the new recovery wrap comes from.)

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::rk_ops::{make_sheet, show_sheet};
use super::setup::{emit_panel, PANEL_TIMEOUT_PUB as PANEL_TIMEOUT};
use super::{lock_core, Deps, OpOutcome, PanelOutcome, PanelRequest, VaultCore};
use crate::crypto::hex;
use crate::crypto::secret::random_secret;
use crate::device::creds::DeviceCreds;
use crate::device::envelope;
use crate::device::identity::SeDevice;
use crate::device::rotate::EnvelopePlan;
use crate::errors::ErrorCode;
use crate::registry::build;
use crate::registry::chain::EpochPolicy;
use crate::registry::device::DeviceIdentity;
use crate::registry::log;
use crate::state::VaultState;
use crate::storage::rotation::{self, MpWrap, RkWrap};
use crate::storage::VaultStore;

const POLICY: EpochPolicy<'static> = EpochPolicy::CheckpointAnchored;

/// §1.5 `list_devices`: the verified registry's view, plus which entry is
/// this device. Public material only.
pub fn list_devices(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    let (dir, vault_id) = {
        let c = lock_core(core);
        if !c.state.vk_resident() {
            return OpOutcome::err(ErrorCode::BadState);
        }
        let Some(h) = c.header.as_ref() else {
            return OpOutcome::err(ErrorCode::BadState);
        };
        (c.vault_dir.clone(), h.vault_id.0)
    };
    let state = match log::read_state(&dir, &vault_id, &POLICY) {
        Ok(s) => s,
        Err(e) => return OpOutcome::err(e),
    };
    let me = SeDevice::load(&dir).map(|d| d.device_id()).ok();
    let devices: Vec<Value> = state
        .devices
        .iter()
        .map(|d| {
            // When it was enrolled, from the entry that installed it.
            // Two devices can share a name — a phone re-paired after a
            // removal keeps its model name — so the list needs something
            // that tells them apart.
            let enrolled_at = state
                .entries
                .get(d.installed_seq as usize)
                .and_then(|e| e.enrolled_at);
            json!({
                "device_id": hex::encode(d.device_id),
                "device_name": d.device_name,
                "platform": d.platform,
                "revoked": d.revoked,
                "installed_seq": d.installed_seq,
                "enrolled_at": enrolled_at,
                "self": Some(d.device_id) == me,
            })
        })
        .collect();
    OpOutcome::ok(json!({
        "devices": devices,
        "registry_head": hex::encode(state.head),
        "epoch": state.epoch,
    }))
}

/// §1.5 `revoke_device`: presence → MP → new Recovery Key acknowledged →
/// revoke entry → VK rotation that re-envelopes every surviving device.
pub fn revoke_device(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let Some(target) = frame
        .get("device_id")
        .and_then(Value::as_str)
        .and_then(hex::decode_array::<16>)
    else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    if let Err(e) = super::rk_ops::authorize_pub(core, deps, "Source Vault: remove a device") {
        return super::rk_ops::finish_pub(core, deps, Err(e));
    }
    let (dir, vault_id) = {
        let c = lock_core(core);
        let Some(h) = c.header.as_ref() else {
            return super::rk_ops::finish_pub(core, deps, Err(ErrorCode::BadState));
        };
        (c.vault_dir.clone(), h.vault_id.0)
    };
    let me = match SeDevice::load(&dir) {
        Ok(d) => d,
        Err(e) => return super::rk_ops::finish_pub(core, deps, Err(e)),
    };
    if target == me.device_id() {
        // A device cannot revoke itself: the result would be a vault with
        // no authorizer (§4.4 rule 3).
        return super::rk_ops::finish_pub(core, deps, Err(ErrorCode::InvalidInput));
    }
    let state = match log::read_state(&dir, &vault_id, &POLICY) {
        Ok(s) => s,
        Err(e) => return super::rk_ops::finish_pub(core, deps, Err(e)),
    };
    if state.active_device(&target).is_none() {
        return super::rk_ops::finish_pub(core, deps, Err(ErrorCode::NotFound));
    }

    // MP: needed to re-seal password.wrap under the new VK.
    emit_panel(deps, true, PanelRequest::MpEntry);
    let outcome = deps.panel.run(PanelRequest::MpEntry, PANEL_TIMEOUT);
    emit_panel(deps, false, PanelRequest::MpEntry);
    let PanelOutcome::Submitted(mp) = outcome else {
        return super::rk_ops::finish_pub(core, deps, Err(ErrorCode::PanelCancelled));
    };
    let (pk, next_gen, head) = {
        let c = lock_core(core);
        let Some(store) = c.store.as_ref().filter(|_| c.state == VaultState::Authorizing) else {
            return OpOutcome::err(ErrorCode::BadState);
        };
        match super::recovery_ops::prove_mp(store, &mp) {
            Ok(pk) => (
                pk,
                store.header.manifest_generation + 1,
                store.header.registry_head.0,
            ),
            Err(e) => {
                drop(c);
                return super::rk_ops::finish_pub(core, deps, Err(e));
            }
        }
    };
    drop(mp);

    // The rotation invalidates the current recovery wrap, so the user
    // leaves with a Recovery Key that works — or nothing is committed.
    let rk = random_secret();
    if !show_sheet(deps, &make_sheet(&rk, &vault_id, next_gen, &head, super::SheetReason::DeviceRemoved)) {
        return super::rk_ops::finish_pub(core, deps, Err(ErrorCode::PanelCancelled));
    }

    match commit_revocation(core, &dir, &vault_id, &me, target, &pk, &rk, &state) {
        Ok(v) => {
            let out = super::rk_ops::finish_pub(core, deps, Ok(()));
            if out.response["ok"] == Value::Bool(true) {
                return OpOutcome::ok(v);
            }
            out
        }
        Err(e) => {
            let mut c = lock_core(core);
            let events = c.lock(super::LockReason::Fatal);
            drop(c);
            for ev in events {
                deps.events.emit(ev);
            }
            OpOutcome::err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn commit_revocation(
    core: &Arc<Mutex<VaultCore>>,
    dir: &std::path::Path,
    vault_id: &[u8; 16],
    me: &SeDevice,
    target: [u8; 16],
    pk: &crate::crypto::secret::SecretBytes<32>,
    rk: &crate::crypto::secret::SecretBytes<32>,
    state: &crate::registry::chain::RegistryState,
) -> Result<Value, ErrorCode> {
    // 1. The registry entry first (§11.4: "after writing the registry
    //    revoke entry"), so even a failed rotation leaves the device
    //    untrusted rather than trusted.
    let entry = build::revoke(state, me, target)?;
    let head = crate::crypto::registry::entry_hash(&entry).map_err(|_| ErrorCode::Internal)?;
    let after = log::append(dir, vault_id, state, entry, &POLICY)?;

    // 2. The revoked device keeps no envelope and no credential here.
    envelope::remove_envelope(dir, &target);

    let mut c = lock_core(core);
    if c.state != VaultState::Authorizing {
        return Err(ErrorCode::BadState);
    }
    let (Some(mut store), Some(vk)) = (c.store.take(), c.vk.take()) else {
        return Err(ErrorCode::BadState);
    };
    store.set_registry_head(head)?;
    let mut creds = DeviceCreds::load(dir, &vk, vault_id)?;
    creds.remove(&target);

    // 3. Rotate. Every surviving device is re-enveloped in the same
    //    journal transaction as the wraps (§2.10 + §11.4).
    let devices: Vec<([u8; 16], [u8; 65])> = after
        .devices
        .iter()
        .filter(|d| !d.revoked)
        .map(|d| (d.device_id, d.agree_pub))
        .collect();
    let plan = EnvelopePlan {
        vault_id: *vault_id,
        devices,
        creds: &creds,
    };
    let rotated = rotation::rotate(
        store,
        &vk,
        MpWrap::Reseal(pk),
        RkWrap::Seal(rk),
        Some(&plan),
        None,
    );
    drop(vk);
    let (outcome, store) = match rotated.and_then(|r| VaultStore::open(dir).map(|s| (r, s))) {
        Ok(v) => v,
        Err(_) => return Err(ErrorCode::RotationFailed),
    };
    let generation = store.header.manifest_generation;
    c.header = Some(store.header.clone());
    c.store = Some(store);
    c.vk = Some(outcome.new_vk.mlock_best_effort());
    let _ = crate::keychain::write_seen_generation(generation);
    Ok(json!({
        "device_id": hex::encode(target),
        "registry_head": hex::encode(head),
        "vk_generation": outcome.vk_generation,
        "manifest_generation": outcome.manifest_generation,
    }))
}
