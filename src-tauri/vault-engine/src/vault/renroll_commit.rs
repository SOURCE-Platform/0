//! Reverse enrollment, the joining Mac's last steps (design §3 steps 6–8,
//! §5):
//!
//! - `renroll_complete {session}`: the bundle, checked in the §22.10 order
//!   against the SAS-confirmed anchor (the iPhone's id and key from the
//!   hello), the VK from the bundle's `password.wrap` opened with the
//!   master password typed in the "Join a Vault" panel; the vault is built
//!   in the staging directory, its VK commitment signed, the floor set,
//!   the ACK signed. A wrong password, or any failed check, ends the
//!   attempt — nothing is kept.
//! - `renroll_finish {registry_head}`: the iPhone committed the entry
//!   (`done`, behind the route proof): the staging moves into the vault
//!   root; the Mac is LOCKED. This is the only way a joined vault appears.
//! - `renroll_abort`: the attempt and its staging go.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::renroll_ops::{staging, wipe_staging};
use super::{lock_core, OpOutcome, VaultCore};
use crate::crypto::hex;
use crate::device::identity::SeDevice;
use crate::enroll::{transcript, wire};
use crate::errors::ErrorCode;
use crate::state::VaultState;
use crate::sync::materialize::{materialize_keyed, Anchor, VkSource};

fn run(f: impl FnOnce() -> Result<Value, ErrorCode>) -> OpOutcome {
    f().map_or_else(OpOutcome::err, OpOutcome::ok)
}

pub fn renroll_complete(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    let id = match frame.get("session").and_then(Value::as_str).and_then(hex::decode_array::<16>) {
        Some(id) => id,
        None => return OpOutcome::err(ErrorCode::InvalidInput),
    };
    // Taken out under the lock; the checks and the Argon2 run without it.
    let (mut session, dir) = {
        let mut c = lock_core(core);
        let ours = c.provider.renroll.as_ref().is_some_and(|s| !s.expired() && s.mp.is_some() && s.acked.is_none() && s.transfer.as_ref().is_some_and(|t| t.id == id));
        if c.phone || c.state != VaultState::Uninitialized || !ours {
            return OpOutcome::err(ErrorCode::TransferInvalid);
        }
        (c.provider.renroll.take().expect("checked above"), c.vault_dir.clone())
    };
    match complete(&dir, &session) {
        Ok((answer, head)) => {
            session.acked = Some(head);
            session.mp = None;
            session.transfer = None;
            lock_core(core).provider.renroll = Some(session);
            OpOutcome::ok(answer)
        }
        Err(e) => {
            drop(session);
            wipe_staging(&dir);
            OpOutcome::err(e)
        }
    }
}

fn complete(dir: &std::path::Path, s: &super::renroll_ops::RenrollSession) -> Result<(Value, [u8; 32]), ErrorCode> {
    let fixed = s.fixed.as_ref().ok_or(ErrorCode::BadState)?;
    let mp = s.mp.as_ref().ok_or(ErrorCode::BadState)?;
    let bytes = s.transfer.as_ref().and_then(|t| t.received.values().next().cloned()).ok_or(ErrorCode::TransferInvalid)?;
    let bundle: wire::Bundle = serde_json::from_slice(&bytes).map_err(|_| ErrorCode::FormatInvalid)?;
    let at = staging(dir);
    let me = SeDevice::load(&at)?;
    let anchor = Anchor {
        me: &me,
        mac_device_id: fixed.authorizer_device_id,
        mac_key: fixed.authorizer_key,
        vault_id: fixed.vault_id,
        nonce_e: fixed.nonce_e,
    };
    let (store, vk, head) = materialize_keyed(&at, &bundle, &anchor, VkSource::Password(mp.as_slice()))?;
    // A key verified through the §22.10 order is one this device trusts
    // (§22.4: a verified first materialization); its commitment is signed
    // before anything else can use it.
    super::vk_commit::record(&at, &fixed.vault_id, store.header.vk_generation, &vk)?;
    drop(vk);
    use crate::registry::device::DeviceIdentity;
    let sig = me.sign_prehash(&transcript::ack_digest(&head, &fixed.authorizer_device_id)).map_err(|_| ErrorCode::DeviceNotAuthorized)?;
    Ok((json!({ "ack": { "signature": hex::encode(sig) }, "registry_head": hex::encode(head) }), head))
}

pub fn renroll_finish(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let head = frame.get("registry_head").and_then(Value::as_str).and_then(hex::decode_array::<32>).ok_or(ErrorCode::InvalidInput)?;
        let mut c = lock_core(core);
        let ok = c.provider.renroll.as_ref().is_some_and(|s| !s.expired() && s.acked == Some(head));
        if c.phone || c.state != VaultState::Uninitialized || !ok {
            return Err(ErrorCode::BadState);
        }
        let dir = c.vault_dir.clone();
        let from = staging(&dir);
        for e in std::fs::read_dir(&from).map_err(|_| ErrorCode::Internal)?.flatten() {
            std::fs::rename(e.path(), dir.join(e.file_name())).map_err(|_| ErrorCode::Internal)?;
        }
        let _ = std::fs::remove_dir(&from);
        c.provider.renroll = None;
        *c = VaultCore::boot(dir);
        // The floor for this vault on this Mac (design §5 step 6).
        // (The signed provider floor from the bundle rides in `kv`, as on
        // the phone: `materialize::JOIN_FLOOR_KEY`.)
        let store = crate::storage::VaultStore::open(&c.vault_dir)?;
        super::floor::join(&store)?;
        Ok(json!({ "state": c.state.as_str() }))
    })
}

pub fn renroll_abort(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    let dir = {
        let mut c = lock_core(core);
        if c.phone {
            return OpOutcome::err(ErrorCode::UnknownOp);
        }
        c.provider.renroll = None;
        c.vault_dir.clone()
    };
    if !dir.join(crate::VAULT_HEADER_NAME).exists() {
        wipe_staging(&dir);
    }
    OpOutcome::ok(json!({}))
}
