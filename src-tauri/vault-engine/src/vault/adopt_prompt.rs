//! The Touch ID prompt of an adoption, outside the core mutex (review
//! SEC-I5). Adopting another device's key change opens this device's
//! envelope in the served state, and a Touch-ID-bound key asks for the
//! finger as it does; held under the mutex, that prompt would block lock,
//! auto-lock and every other op until answered. So `backup_apply` opens
//! the envelope first, unlocked, and the apply under the mutex uses that
//! result — for exactly the envelope file it was opened from. It does so
//! only for a state signed by a device this Mac's confirmed registry
//! knows; any other state is fully verified before any prompt.
//!
//! When this device's own key cannot open its envelope
//! (`DEVICE_NOT_AUTHORIZED`: an agreement key discarded by the owner
//! decision "password every time", or Touch ID unavailable now), the
//! master password opens the served `wrap_mp` instead (F.2d, §2.7) —
//! asked in the same secure panel, also outside the mutex.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use super::secure_ui::{PanelOutcome, PanelRequest};
use super::setup::emit_panel;
use super::setup::PANEL_TIMEOUT_PUB as PANEL_TIMEOUT;
use super::{lock_core, Deps, VaultCore};
use crate::backup::index::Role;
use crate::crypto::wrap::DeviceEnvelopePayload;
use crate::device::envelope::{self, DeviceEnvelopeFile};
use crate::device::SeDevice;
use crate::errors::ErrorCode;
use crate::registry::device::DeviceIdentity;
use crate::sync::session::Id;

pub const REASON: &str = "Source Vault: apply a security change from another device";

/// An envelope opened ahead of the apply, keyed by its exact bytes.
pub struct Opened {
    file: Vec<u8>,
    result: RefCell<Option<Result<DeviceEnvelopePayload, ErrorCode>>>,
}

impl Opened {
    /// The pre-opened result for `f`; `None` for any other file (the
    /// caller then opens it itself).
    pub fn take(&self, f: &DeviceEnvelopeFile) -> Option<Result<DeviceEnvelopePayload, ErrorCode>> {
        (serde_json::to_vec(f).ok()? == self.file).then(|| self.result.borrow_mut().take()).flatten()
    }
}

/// When the complete sync session `id` will need this device's envelope
/// (the served key generation differs, or a local rotation is pending),
/// open it now, with the mutex released. `interactive`: the user started
/// this sync; a background one never raises the master-password panel
/// (review SEC-I3) and answers `MP_ADOPTION_REQUIRED` instead.
pub fn pre_open(core: &Arc<Mutex<VaultCore>>, id: &Id, deps: &Deps, interactive: bool) -> Option<Opened> {
    let (tag, vid, file, wrap) = {
        let c = lock_core(core);
        let session = c.provider.sync.as_ref().filter(|s| &s.t.id == id)?;
        let index = session.index.as_ref().filter(|_| session.t.still_needed().is_empty())?;
        let store = c.store.as_ref()?;
        let local = store.header.vk_generation;
        let base = crate::sync::pending::load(&store.conn).ok()?.map_or(local, |p| p.base.vk_generation);
        if session.remote.vk_generation == local && base == local {
            return None; // our own key: no envelope is opened
        }
        // Never ask — for a finger or a password — over a state that does
        // not pass the apply's own verification (reviews VER-I17, SEC-B1):
        // registry extends ours and verifies, signer active, manifest
        // signed, this device listed; when behind, also anchored on the
        // floor (§22.14: the restored registry is no anchor). Pure checks,
        // under the mutex; the prompts below run without it.
        let me = SeDevice::load(&c.vault_dir).ok()?;
        let vk = c.vk.as_ref()?;
        let served = crate::sync::served::verify(store, vk, &session.remote, index, &session.t.received, me.device_id()).ok()?;
        if served.revoked || (c.behind && !super::floor::anchored(store, &served.remote_entries).unwrap_or(false)) {
            return None;
        }
        let blob = crate::sync::served::blob(index, &session.t.received, &Role::Env { device_id: me.device_id() }).ok()?;
        let file: DeviceEnvelopeFile = serde_json::from_slice(blob).ok()?;
        (me.key_tag().to_string(), store.header.vault_id.0, file, crate::sync::adopt_mp::checked_wrap(index, &session.t.received).ok())
    };
    let mut result = envelope::open_envelope_for(&tag, REASON, &vid, &file);
    if let (Err(ErrorCode::DeviceNotAuthorized), Some(wrap)) = (&result, wrap) {
        result = if !interactive {
            Err(ErrorCode::MpAdoptionRequired)
        } else {
            emit_panel(deps, true, PanelRequest::MpAdopt);
            let outcome = deps.panel.run(PanelRequest::MpAdopt, PANEL_TIMEOUT);
            emit_panel(deps, false, PanelRequest::MpAdopt);
            match outcome {
                PanelOutcome::Submitted(mp) => crate::sync::adopt_mp::open_with_mp(&wrap, &vid, &mp),
                _ => Err(ErrorCode::PanelCancelled),
            }
        };
        // §15 / §22.4: a wrong password at this gate drives the backoff
        // too (review VER-B1); the mutex is not held here.
        if let Err(e) = &result {
            super::recovery_ops::backoff(core, *e);
        }
    }
    Some(Opened { file: serde_json::to_vec(&file).ok()?, result: RefCell::new(Some(result)) })
}
