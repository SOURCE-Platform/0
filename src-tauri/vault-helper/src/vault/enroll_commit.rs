//! The committing half of enrollment (§5.1 steps after the SAS match):
//! building the transfer bundle, and the ACK that writes the registry
//! entry.
//!
//! Nothing here is reachable before the user has compared the SAS on
//! both screens and passed the Mac's presence check, and nothing becomes
//! part of the registry until the phone has proved — with a signature
//! under the key it presented — that it holds the matching private key.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::{lock_core, OpOutcome, VaultCore};
use crate::crypto::secret::{random_secret, SecretBytes};
use crate::crypto::wrap::DeviceEnvelopePayload;
use crate::crypto::{ecdsa, hex};
use crate::device::creds::DeviceCreds;
use crate::device::envelope;
use crate::device::identity::SeDevice;
use crate::enroll::session::{Peer, Stage};
use crate::enroll::{transcript, wire};
use crate::errors::ErrorCode;
use crate::registry::build;
use crate::registry::device::DeviceIdentity;
use crate::registry::log;
use crate::storage::store::now_epoch;

use super::enroll_ops::{require_unlocked, POLICY};

pub(super) fn build_bundle(core: &Arc<Mutex<VaultCore>>) -> Result<Value, ErrorCode> {
    let mut c = lock_core(core);
    let dir = c.vault_dir.clone();
    let vk = c
        .vk
        .as_ref()
        .map(|v| SecretBytes::new(*v.expose()))
        .ok_or(ErrorCode::BadState)?;
    let store = c.store.as_ref().ok_or(ErrorCode::BadState)?;
    let vault_id = store.header.vault_id.0;
    let vk_generation = store.header.vk_generation;
    let me = SeDevice::load(&dir)?;
    let peer = c
        .enroll
        .as_ref()
        .and_then(|s| s.peer.clone())
        .ok_or(ErrorCode::BadState)?;

    // The enroll entry (signed, not yet appended) and the chain the phone
    // must verify.
    let state = log::read_state(&dir, &vault_id, &POLICY)?;
    let entry = build::enroll(&state, &me, &PeerIdentity(&peer))?;
    let head = crate::crypto::registry::entry_hash(&entry).map_err(|_| ErrorCode::Internal)?;
    let mut entries = state.entries.clone();
    entries.push(entry.clone());

    // The device's own envelope: VK plus the credential this Mac issues
    // to it (§2.2, §11.4 class 1).
    let cred = random_secret();
    let nonce_e = c.enroll.as_ref().map(|s| s.nonce_e).ok_or(ErrorCode::BadState)?;
    let payload = DeviceEnvelopePayload {
        vk: SecretBytes::new(*vk.expose()),
        device_backup_cred: SecretBytes::new(*cred.expose()),
        wrapped_at: now_epoch(),
        vk_generation,
    };
    let env = envelope::seal_envelope(&peer.agree_pub, &vault_id, &peer.device_id, &nonce_e, &payload)?;

    // The vault itself, as a §11.2 snapshot signed by this Mac.
    let snap = crate::backup::snapshot::build(
        c.store.as_ref().ok_or(ErrorCode::BadState)?,
        &entries,
        store.header.manifest_generation,
        [0u8; 32],
        &me,
        &vk,
    )?;
    let bundle = wire::Bundle {
        vault_id: hex::encode(vault_id),
        objects: snap
            .objects
            .iter()
            .map(|(k, v)| (k.clone(), hex::encode(v)))
            .collect(),
        manifest: hex::encode(snap.manifest.encode()),
        checkpoint: hex::encode(snap.checkpoint.encode()),
        envelope: serde_json::to_value(&env).map_err(|_| ErrorCode::Internal)?,
        registry_head: hex::encode(head),
    };
    let session = c.enroll.as_mut().ok_or(ErrorCode::BadState)?;
    session.entry = Some(entry);
    session.cred = Some(cred);
    session.stage = Stage::AwaitingAck;
    serde_json::to_value(&bundle).map_err(|_| ErrorCode::Internal)
}

/// §5.2 ENROLL_ACK: the phone's signature over the registry head proves
/// its SE signing key exists on that device. Only now does the entry
/// become part of the chain (a cancelled or failed attempt leaves no
/// registry trace).
pub fn enroll_ack(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    if let Err(e) = require_unlocked(core) {
        return OpOutcome::err(e);
    }
    let Some(sig) = frame.get("signature").and_then(Value::as_str).and_then(hex::decode_array::<64>)
    else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    match finish_ack(core, &sig) {
        Ok(v) => OpOutcome::ok(v),
        Err(e) => {
            if e == ErrorCode::SignatureInvalid {
                lock_core(core).enroll = None; // §5.3: no retry on a bad ACK
            }
            OpOutcome::err(e)
        }
    }
}

fn finish_ack(core: &Arc<Mutex<VaultCore>>, sig: &[u8; 64]) -> Result<Value, ErrorCode> {
    let mut c = lock_core(core);
    let dir = c.vault_dir.clone();
    let vk = c
        .vk
        .as_ref()
        .map(|v| SecretBytes::new(*v.expose()))
        .ok_or(ErrorCode::BadState)?;
    let vault_id = c.store.as_ref().ok_or(ErrorCode::BadState)?.header.vault_id.0;
    let me = SeDevice::load(&dir)?;
    let session = c.enroll.as_ref().ok_or(ErrorCode::BadState)?;
    if session.stage != Stage::AwaitingAck || session.expired() {
        return Err(ErrorCode::BadState);
    }
    let peer = session.peer.clone().ok_or(ErrorCode::BadState)?;
    let entry = session.entry.clone().ok_or(ErrorCode::BadState)?;
    let cred = session
        .cred
        .as_ref()
        .map(|c| SecretBytes::new(*c.expose()))
        .ok_or(ErrorCode::BadState)?;
    let head = crate::crypto::registry::entry_hash(&entry).map_err(|_| ErrorCode::Internal)?;
    let digest = transcript::ack_digest(&head, &me.device_id());
    ecdsa::verify_prehash(&peer.sign_pub, &digest, sig)
        .map_err(|_| ErrorCode::SignatureInvalid)?;

    // Commit: entry → registry, envelope copy → disk (so a later VK
    // rotation can re-seal it), credential → the issuer's record.
    let state = log::read_state(&dir, &vault_id, &POLICY)?;
    log::append(&dir, &vault_id, &state, entry, &POLICY)?;
    let env_file = envelope::seal_envelope(
        &peer.agree_pub,
        &vault_id,
        &peer.device_id,
        &session.nonce_e,
        &DeviceEnvelopePayload {
            vk: SecretBytes::new(*vk.expose()),
            device_backup_cred: SecretBytes::new(*cred.expose()),
            wrapped_at: now_epoch(),
            vk_generation: c.store.as_ref().ok_or(ErrorCode::BadState)?.header.vk_generation,
        },
    )?;
    envelope::write_envelope(&dir, &peer.device_id, &env_file)?;
    let mut creds = DeviceCreds::load(&dir, &vk, &vault_id)?;
    creds.insert(peer.device_id, &cred);
    creds.save(&dir, &vk, &vault_id)?;
    let store = c.store.as_mut().ok_or(ErrorCode::BadState)?;
    store.set_registry_head(head)?;
    let header = store.header.clone();
    c.header = Some(header);
    c.enroll = None;
    Ok(json!({
        "device_id": hex::encode(peer.device_id),
        "device_name": peer.device_name,
        "registry_head": hex::encode(head),
    }))
}

/// The peer's public identity as `build::enroll` needs it. It never
/// signs: only the authorizing Mac does.
struct PeerIdentity<'a>(&'a Peer);

impl DeviceIdentity for PeerIdentity<'_> {
    fn device_id(&self) -> [u8; 16] {
        self.0.device_id
    }
    fn device_name(&self) -> String {
        self.0.device_name.clone()
    }
    fn platform(&self) -> u8 {
        self.0.platform
    }
    fn sign_pub(&self) -> [u8; 65] {
        self.0.sign_pub
    }
    fn agree_pub(&self) -> [u8; 65] {
        self.0.agree_pub
    }
    fn sign_prehash(&self, _: &[u8; 32]) -> Result<[u8; 64], crate::crypto::CryptoError> {
        Err(crate::crypto::CryptoError::SignatureInvalid)
    }
}
