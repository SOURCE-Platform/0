//! Reverse enrollment, the iPhone's side (spec v0.5 §22.13; design
//! `phase-f2e-design.md` revision 2, §3 and §6): the iPhone adds a
//! **replacement** Mac. Nothing is committed before the Mac's ACK; a lock
//! ends the attempt on both sides (the Mac discards without `done`).
//!
//! - `authorize_begin {qr}` → `{hello, host, port, fp}`, after the
//!   preconditions (before any prompt): UNLOCKED, not behind, not removed,
//!   no rotation waiting (`ROTATION_PENDING`), no other active Mac and no
//!   Mac revocation still waiting (`REPLACEMENT_ONLY`), a verified provider
//!   exchange in the last 15 minutes (`NOT_FRESH`).
//! - `authorize_reply {reply}` → `{sas}`: the reply must open the QR's
//!   commitment, name a macOS device and carry valid, distinct keys.
//! - `authorize_confirm` → presence and the master password (§22.4), then
//!   the signed entry, the envelope and the bundle, streamed out
//!   (`{session, stream, size, proof}`).
//! - `authorize_finish {ack, peer_endpoint}` → the ACK verified, the entry
//!   appended, the envelope kept, the pending `Enrollment` recorded, the
//!   Mac's `peer_endpoint` kept with its id → `{registry_head, proof}` for
//!   the `done` route.
//! - `authorize_abort`.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value};

use super::{lock_core, Deps, OpOutcome, PanelOutcome, PanelRequest, VaultCore};
use crate::crypto::registry::RegistryEntry;
use crate::crypto::secret::SecretBytes;
use crate::crypto::wrap::DeviceEnvelopePayload;
use crate::crypto::{ecdsa, hex};
use crate::device::envelope;
use crate::device::identity::SeDevice;
use crate::enroll::reverse::{self, Hello, ParsedQr, Qr, Reply};
use crate::enroll::session::{Peer, SESSION_TTL};
use crate::enroll::transcript::{ack_digest, sas};
use crate::errors::ErrorCode;
use crate::registry::device::{DeviceIdentity, PLATFORM_MACOS};
use crate::registry::log;
use crate::state::VaultState;
use crate::storage::store::now_epoch;
use crate::sync::pending::{self, PendingOp};

pub struct AuthorizeSession {
    qr: ParsedQr,
    nonce_e: [u8; 16],
    new_device_id: [u8; 16],
    vault_id: [u8; 16],
    started: Instant,
    peer: Option<Peer>,
    sas: Option<String>,
    entry: Option<(RegistryEntry, [u8; 32])>,
    /// The bundle, read by Swift with `stream_read`.
    pub bundle: Option<crate::sync::session::Transfer>,
}

impl AuthorizeSession {
    fn expired(&self) -> bool {
        self.started.elapsed() >= SESSION_TTL
    }
    fn proof(&self) -> [u8; 32] {
        reverse::route_proof(&self.qr.secret, &self.nonce_e)
    }
}

impl Drop for AuthorizeSession {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.qr.secret.zeroize();
    }
}

fn run(f: impl FnOnce() -> Result<Value, ErrorCode>) -> OpOutcome {
    f().map_or_else(OpOutcome::err, OpOutcome::ok)
}

/// Design §6, checked before any prompt.
fn preconditions(c: &VaultCore) -> Result<(), ErrorCode> {
    if !c.phone {
        return Err(ErrorCode::UnknownOp);
    }
    if c.state != VaultState::Unlocked || c.unverified_key {
        return Err(ErrorCode::BadState);
    }
    if c.behind {
        return Err(ErrorCode::VaultBehind);
    }
    if crate::peer::client::removal::active(true, &c.vault_dir).is_some() {
        return Err(ErrorCode::PeerNotPermitted);
    }
    let store = c.store.as_ref().ok_or(ErrorCode::BadState)?;
    if pending::rotation_waiting(&store.conn, &store.header)? {
        return Err(ErrorCode::RotationPending);
    }
    // Replacement only (design §1): the lost Mac's revocation committed.
    if pending::load(&store.conn)?.is_some_and(|p| p.all_ops().contains(&PendingOp::Revocation)) {
        return Err(ErrorCode::ReplacementOnly);
    }
    let me = SeDevice::load(&c.vault_dir)?.device_id();
    let reg = log::read_state(&c.vault_dir, &store.header.vault_id.0, &super::enroll_ops::POLICY)?;
    if reg.devices.iter().any(|d| !d.revoked && d.platform == PLATFORM_MACOS && d.device_id != me) {
        return Err(ErrorCode::ReplacementOnly);
    }
    if !c.provider_checked.as_ref().is_some_and(|t| t.fresh()) {
        return Err(ErrorCode::NotFresh);
    }
    Ok(())
}

pub fn authorize_begin(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let qr: Qr = serde_json::from_value(frame.get("qr").cloned().ok_or(ErrorCode::InvalidInput)?).map_err(|_| ErrorCode::InvalidInput)?;
        let parsed = reverse::parse_qr(&qr)?;
        let mut c = lock_core(core);
        preconditions(&c)?;
        let me = SeDevice::load(&c.vault_dir)?;
        let vault_id = c.store.as_ref().ok_or(ErrorCode::BadState)?.header.vault_id.0;
        let reg = log::read_state(&c.vault_dir, &vault_id, &super::enroll_ops::POLICY)?;
        let mut new_device_id = crate::registry::device::random_uuid();
        while reg.devices.iter().any(|d| d.device_id == new_device_id) {
            new_device_id = crate::registry::device::random_uuid();
        }
        let mut nonce_e = [0u8; 16];
        getrandom::fill(&mut nonce_e).map_err(|_| ErrorCode::Internal)?;
        let hello = Hello {
            v: reverse::PROTO_V3,
            secret: qr.secret.clone(),
            nonce_e: hex::encode(nonce_e),
            authorizer_device_id: hex::encode(me.device_id()),
            authorizer_sign_pub: hex::encode(me.sign_pub()),
            new_device_id: hex::encode(new_device_id),
            vault_id: hex::encode(vault_id),
        };
        c.provider.authorize = Some(AuthorizeSession { qr: parsed, nonce_e, new_device_id, vault_id, started: Instant::now(), peer: None, sas: None, entry: None, bundle: None });
        Ok(json!({ "hello": hello, "host": qr.host, "port": qr.port, "fp": qr.fp }))
    })
}

pub fn authorize_reply(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let reply: Reply = serde_json::from_value(frame.get("reply").cloned().ok_or(ErrorCode::InvalidInput)?).map_err(|_| ErrorCode::InvalidInput)?;
        let mut c = lock_core(core);
        let me = SeDevice::load(&c.vault_dir)?;
        let s = c.provider.authorize.as_mut().filter(|s| !s.expired() && s.peer.is_none()).ok_or(ErrorCode::BadState)?;
        let bad = || ErrorCode::ProtocolViolation;
        let nonce_n = hex::decode_array::<16>(&reply.nonce_n).ok_or_else(bad)?;
        let sign_pub = hex::decode_array::<65>(&reply.sign_pub).ok_or_else(bad)?;
        let agree_pub = hex::decode_array::<65>(&reply.agree_pub).ok_or_else(bad)?;
        if reverse::commitment(&nonce_n, &sign_pub, &agree_pub, &reply.name, reply.platform) != s.qr.commit {
            return Err(ErrorCode::ProtocolViolation); // the reply must open the QR's commitment
        }
        ecdsa::parse_verifying_key(&sign_pub).map_err(|_| bad())?;
        ecdsa::parse_verifying_key(&agree_pub).map_err(|_| bad())?;
        if sign_pub == agree_pub || reply.platform != PLATFORM_MACOS || reply.name.is_empty() || reply.name.chars().count() > 64 {
            return Err(ErrorCode::ProtocolViolation);
        }
        let authorizer_key = reverse::authorizer_key(&me.sign_pub());
        let t = reverse::transcript(&reverse::Binding {
            fp: &s.qr.fp,
            authorizer_key: &authorizer_key,
            secret: &s.qr.secret,
            nonce_e: &s.nonce_e,
            nonce_n: &nonce_n,
            authorizer_device_id: &me.device_id(),
            new_device_id: &s.new_device_id,
            sign_pub: &sign_pub,
            agree_pub: &agree_pub,
            vault_id: &s.vault_id,
        });
        let code = sas(&t)?;
        s.peer = Some(Peer { device_id: s.new_device_id, device_name: reply.name, platform: PLATFORM_MACOS, sign_pub, agree_pub });
        s.sas = Some(code.clone());
        Ok(json!({ "sas": code }))
    })
}

/// The user compared the codes and tapped "They match": presence and the
/// master password (§22.4), then the entry, envelope and bundle.
pub fn authorize_confirm(core: &Arc<Mutex<VaultCore>>, deps: &Deps) -> OpOutcome {
    {
        let c = lock_core(core);
        if let Err(e) = preconditions(&c) {
            return OpOutcome::err(e);
        }
        if !c.provider.authorize.as_ref().is_some_and(|s| !s.expired() && s.sas.is_some() && s.entry.is_none()) {
            return OpOutcome::err(ErrorCode::BadState);
        }
    }
    if let Err(e) = super::rk_ops::authorize_pub(core, deps, "Source Vault: add a Mac to your vault") {
        return super::rk_ops::finish_pub(core, deps, Err(e));
    }
    super::setup::emit_panel(deps, true, PanelRequest::MpEntry);
    let outcome = deps.panel.run(PanelRequest::MpEntry, super::setup::PANEL_TIMEOUT_PUB);
    super::setup::emit_panel(deps, false, PanelRequest::MpEntry);
    let PanelOutcome::Submitted(mp) = outcome else {
        return super::rk_ops::finish_pub(core, deps, Err(ErrorCode::PanelCancelled));
    };
    let proven = super::recovery_ops::prove_mp_resident(&lock_core(core), &mp).map(|_| ());
    drop(mp);
    if let Err(e) = proven {
        super::recovery_ops::backoff(core, e);
        return super::rk_ops::finish_pub(core, deps, Err(e));
    }
    let done = super::rk_ops::finish_pub(core, deps, Ok(()));
    if done.response["ok"] != Value::Bool(true) {
        return done;
    }
    run(|| {
        let mut c = lock_core(core);
        let s = c.provider.authorize.as_ref().filter(|s| !s.expired()).ok_or(ErrorCode::BadState)?;
        let (peer, nonce_e, proof) = (s.peer.clone().ok_or(ErrorCode::BadState)?, s.nonce_e, s.proof());
        let (bundle, entry, head) = super::enroll_commit::bundle_for(&c, &peer, &nonce_e)?;
        let bytes = serde_json::to_vec(&bundle).map_err(|_| ErrorCode::Internal)?;
        let sha: [u8; 32] = <sha2::Sha256 as sha2::Digest>::digest(&bytes).into();
        let size = bytes.len();
        let t = crate::sync::session::Transfer::new([(sha, bytes)].into_iter().collect());
        let out = json!({ "session": hex::encode(t.id), "stream": hex::encode(sha), "size": size, "proof": hex::encode(proof) });
        let s = c.provider.authorize.as_mut().ok_or(ErrorCode::BadState)?;
        s.entry = Some((entry, head));
        s.bundle = Some(t);
        Ok(out)
    })
}

pub fn authorize_finish(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let sig = frame.get("ack").and_then(|a| a.get("signature")).and_then(Value::as_str).and_then(hex::decode_array::<64>).ok_or(ErrorCode::InvalidInput)?;
        // Absent (or null) when the Mac's mobile server is off: no peer sync yet.
        let endpoint = frame.get("peer_endpoint").filter(|v| !v.is_null()).map(super::join_ops::peer_endpoint_checked).transpose()?;
        let mut c = lock_core(core);
        if c.state != VaultState::Unlocked {
            return Err(ErrorCode::BadState);
        }
        let dir = c.vault_dir.clone();
        let me = SeDevice::load(&dir)?;
        let s = c.provider.authorize.take().filter(|s| !s.expired()).ok_or(ErrorCode::BadState)?;
        let (peer, (entry, head)) = (s.peer.clone().ok_or(ErrorCode::BadState)?, s.entry.clone().ok_or(ErrorCode::BadState)?);
        ecdsa::verify_prehash(&peer.sign_pub, &ack_digest(&head, &me.device_id()), &sig).map_err(|_| ErrorCode::SignatureInvalid)?;
        let vk = c.vk.as_ref().map(|v| SecretBytes::new(*v.expose())).ok_or(ErrorCode::BadState)?;
        let store = c.store.as_ref().ok_or(ErrorCode::BadState)?;
        let vault_id = store.header.vault_id.0;
        let state = log::read_state(&dir, &vault_id, &super::enroll_ops::POLICY)?;
        let base = pending::base_for(&store.conn, &store.header)?;
        let env = envelope::seal_envelope(&peer.agree_pub, &vault_id, &peer.device_id, &s.nonce_e, &DeviceEnvelopePayload { vk, wrapped_at: now_epoch(), vk_generation: store.header.vk_generation })?;
        log::append(&dir, &vault_id, &state, entry, &super::enroll_ops::POLICY)?;
        envelope::write_envelope(&dir, &peer.device_id, &env)?;
        let store = c.store.as_mut().ok_or(ErrorCode::BadState)?;
        store.set_registry_head(head)?;
        pending::add(&store.conn, PendingOp::Enrollment, false, base, Vec::new(), now_epoch())?;
        c.header = Some(c.store.as_ref().ok_or(ErrorCode::BadState)?.header.clone());
        // The new Mac's peer endpoint, bound to its id (design §7).
        if let Some(mut ep) = endpoint {
            ep["device_id"] = json!(hex::encode(peer.device_id));
            crate::keychain::upsert_item(super::join_ops::PEER_ENDPOINT_ITEM, &serde_json::to_vec(&ep).map_err(|_| ErrorCode::Internal)?)?;
        }
        Ok(json!({ "registry_head": hex::encode(head), "proof": hex::encode(s.proof()), "device_id": hex::encode(peer.device_id) }))
    })
}

pub fn authorize_abort(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    lock_core(core).provider.authorize = None;
    OpOutcome::ok(json!({}))
}
