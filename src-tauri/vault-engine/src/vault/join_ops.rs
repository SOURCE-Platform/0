//! The phone's enrollment ops (spec §5, v0.5 §22.10; FFI catalogue §4):
//! SOURCE Vault joins a vault its Mac authorizes. Swift carries each frame
//! over TLS pinned to the QR's `fp`; every decision is here.
//!
//! - `join_begin {qr, name}` → this device's Secure Enclave keys and the
//!   hello to send, plus where to send it;
//! - `join_hello {reply}` → the SAS to compare with the Mac's Source Vault
//!   window, and the proof the bundle and ACK routes require;
//! - `join_bundle_begin {sha256, size}` → a session the bundle streams into;
//! - `join_complete {session}` → the §22.10 checks, the vault written,
//!   the ENROLL_ACK signature to send; the vault stays LOCKED;
//! - `join_finish` once the Mac accepted the ACK; `join_abort` otherwise.
//!
//! An attempt marker lives in the vault directory from the first key to
//! `join_finish`. An attempt that never finished — failed, aborted, or
//! interrupted by the app being killed — is removed whole: its files, its
//! Secure Enclave keys and its Keychain items (§5.2 "Cancellation").

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::{lock_core, OpOutcome, VaultCore};
use crate::crypto::hex;
use crate::device::identity::{self, SeDevice};
use crate::enroll::join::{self, JoinStage, Qr};
use crate::enroll::{transcript, wire};
use crate::errors::ErrorCode;
use crate::registry::device::{DeviceIdentity, PLATFORM_IOS};
use crate::state::VaultState;
use crate::sync::materialize::{materialize, Anchor};
use crate::sync::session::Transfer;

/// Present from `join_begin` until `join_finish`.
pub const ATTEMPT_MARKER: &str = "join.attempt";
/// `peer_endpoint` (wire annex A.4): pin, token, port and hints, all in the
/// Keychain (`WhenUnlockedThisDeviceOnly`), as A.4 says.
pub const PEER_ENDPOINT_ITEM: &str = "com.racker.zero.vault.peer-endpoint";
/// The largest bundle accepted (hex-encoded objects of a large vault).
pub const MAX_BUNDLE: u64 = 256 << 20;
const REASON: &str = "SOURCE Vault: finish pairing with your Mac";

fn run(f: impl FnOnce() -> Result<Value, ErrorCode>) -> OpOutcome {
    f().map_or_else(OpOutcome::err, OpOutcome::ok)
}

fn uninitialized(c: &VaultCore) -> Result<(), ErrorCode> {
    if c.state == VaultState::Uninitialized { Ok(()) } else { Err(ErrorCode::BadState) }
}

/// At boot: an attempt that never finished is removed (review VER-I1).
pub fn recover_at_boot(dir: &Path) {
    if dir.join(ATTEMPT_MARKER).exists() {
        discard(dir);
    }
}

pub fn join_begin(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let qr: Qr = serde_json::from_value(frame.get("qr").cloned().ok_or(ErrorCode::InvalidInput)?).map_err(|_| ErrorCode::InvalidInput)?;
        let parsed = join::parse(&qr)?; // before any key exists
        let name = frame.get("name").and_then(Value::as_str).unwrap_or("iPhone").chars().take(64).collect::<String>();
        let dir = {
            let c = lock_core(core);
            uninitialized(&c)?;
            c.vault_dir.clone()
        };
        discard(&dir); // whatever an earlier attempt left
        std::fs::write(dir.join(ATTEMPT_MARKER), b"").map_err(|_| ErrorCode::Internal)?;
        let me = SeDevice::create(&dir, &name, PLATFORM_IOS)?;
        if me.agreement_discarded() {
            // No Face ID enrolled: this phone's envelope could never open.
            discard(&dir);
            return Err(ErrorCode::DeviceNotAuthorized);
        }
        let (session, hello) = join::begin(&qr, parsed, &me)?;
        lock_core(core).provider.join = Some(session);
        Ok(json!({ "hello": hello, "host": qr.host, "port": qr.port, "fp": qr.fp, "mac_name": qr.name }))
    })
}

pub fn join_hello(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let reply: wire::HelloReply = serde_json::from_value(frame.get("reply").cloned().ok_or(ErrorCode::InvalidInput)?).map_err(|_| ErrorCode::InvalidInput)?;
        let mut c = lock_core(core);
        let dir = c.vault_dir.clone();
        let mut me = SeDevice::load(&dir)?;
        let s = c.provider.join.as_mut().ok_or(ErrorCode::BadState)?;
        let sas = join::hello_reply(s, &me, reply)?;
        let (new_id, _, _) = s.reply_ids()?;
        let proof = join::route_proof(s);
        me.adopt_id(&dir, new_id)?;
        Ok(json!({ "sas": sas, "proof": hex::encode(proof) }))
    })
}

pub fn join_bundle_begin(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let sha = frame.get("sha256").and_then(Value::as_str).and_then(hex::decode_array::<32>).ok_or(ErrorCode::InvalidInput)?;
        let size = frame.get("size").and_then(Value::as_u64).filter(|n| *n <= MAX_BUNDLE).ok_or(ErrorCode::InvalidInput)?;
        let mut c = lock_core(core);
        let s = c.provider.join.as_mut().filter(|s| s.stage == JoinStage::AwaitingBundle && !s.expired()).ok_or(ErrorCode::BadState)?;
        let mut t = Transfer::new(Default::default());
        t.expect([(sha, size)]);
        let id = t.id;
        s.transfer = Some(t);
        Ok(json!({ "session": hex::encode(id) }))
    })
}

pub fn join_complete(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let id = frame.get("session").and_then(Value::as_str).and_then(hex::decode_array::<16>).ok_or(ErrorCode::InvalidInput)?;
        // Taken out under the lock — only this session, only in time; the
        // Face ID prompt then runs without the lock.
        let (session, dir) = {
            let mut c = lock_core(core);
            uninitialized(&c)?;
            let ours = c.provider.join.as_ref().is_some_and(|s| s.stage == JoinStage::AwaitingBundle && !s.expired() && s.transfer.as_ref().is_some_and(|t| t.id == id));
            if !ours {
                return Err(ErrorCode::TransferInvalid);
            }
            (c.provider.join.take().ok_or(ErrorCode::Internal)?, c.vault_dir.clone())
        };
        match complete(&dir, &session) {
            Ok((answer, header)) => {
                let mut c = lock_core(core);
                c.header = Some(header);
                c.state = VaultState::Locked;
                Ok(answer)
            }
            Err(e) => {
                discard(&dir);
                Err(e)
            }
        }
    })
}

/// Everything after the bundle arrived; any error here is discarded whole
/// by the caller.
fn complete(dir: &Path, session: &join::JoinSession) -> Result<(Value, crate::storage::header::Header), ErrorCode> {
    let bytes = session.transfer.as_ref().and_then(|t| t.received.values().next().cloned()).ok_or(ErrorCode::TransferInvalid)?;
    let raw: Value = serde_json::from_slice(&bytes).map_err(|_| ErrorCode::FormatInvalid)?;
    let bundle: wire::Bundle = serde_json::from_value(raw.clone()).map_err(|_| ErrorCode::FormatInvalid)?;
    let endpoint = raw.get("peer_endpoint").map(peer_endpoint).transpose()?;
    let me = SeDevice::load(dir)?;
    let (_, nonce_e, vault_id) = session.reply_ids()?;
    let anchor = Anchor { me: &me, mac_device_id: session.mac_device_id, mac_key: session.mac_key, vault_id, nonce_e };
    let tag = me.key_tag().to_string();
    let open = move |f: &crate::device::envelope::DeviceEnvelopeFile| crate::device::envelope::open_envelope_for(&tag, REASON, &vault_id, f);
    let (store, vk, head) = materialize(dir, &bundle, &anchor, &open)?;
    // A key verified through §22.10 is one this device trusts: its SE
    // commitment is signed now, as at vault creation (§22.4).
    super::vk_commit::record(dir, &vault_id, store.header.vk_generation, &vk)?;
    drop(vk);
    if let Some(ep) = endpoint {
        crate::keychain::upsert_item(PEER_ENDPOINT_ITEM, &serde_json::to_vec(&ep).map_err(|_| ErrorCode::Internal)?)?;
    }
    super::floor::reset(&store)?;
    let sig = me.sign_prehash(&transcript::ack_digest(&head, &session.mac_device_id)).map_err(|_| ErrorCode::DeviceNotAuthorized)?;
    let answer = json!({ "ack": { "signature": hex::encode(sig) }, "proof": hex::encode(join::route_proof(session)), "registry_head": hex::encode(head) });
    Ok((answer, store.header.clone()))
}

/// The Mac accepted the ACK: the vault is this device's for good.
pub fn join_finish(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    run(|| {
        let c = lock_core(core);
        let marker = c.vault_dir.join(ATTEMPT_MARKER);
        if c.state != VaultState::Locked || !marker.exists() || !c.vault_dir.join(crate::VAULT_HEADER_NAME).exists() {
            return Err(ErrorCode::BadState);
        }
        std::fs::remove_file(marker).map_err(|_| ErrorCode::Internal)?;
        Ok(json!({}))
    })
}

/// Pairing failed or was cancelled: nothing it created survives. Only an
/// unfinished attempt can be removed this way.
pub fn join_abort(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    run(|| {
        let mut c = lock_core(core);
        let dir = c.vault_dir.clone();
        let attempt = dir.join(ATTEMPT_MARKER).exists() || c.provider.join.is_some();
        if !attempt || c.vk.is_some() || !matches!(c.state, VaultState::Uninitialized | VaultState::Locked) {
            return Err(ErrorCode::BadState);
        }
        c.provider.join = None;
        c.store = None;
        c.header = None;
        c.state = VaultState::Uninitialized;
        discard(&dir);
        Ok(json!({}))
    })
}

/// Remove an attempt's vault files, Secure Enclave keys and Keychain items.
fn discard(dir: &Path) {
    identity::wipe(dir);
    crate::keychain::remove_item(PEER_ENDPOINT_ITEM);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let _ = if e.path().is_dir() { std::fs::remove_dir_all(e.path()) } else { std::fs::remove_file(e.path()) };
        }
    }
}

/// Annex A.4, checked before anything is kept: a 64-hex pin, a 32-byte
/// base64url token, a port, at most 8 IP-literal hints.
fn peer_endpoint(ep: &Value) -> Result<Value, ErrorCode> {
    let bad = || ErrorCode::FormatInvalid;
    let pin = ep.get("spki_sha256").and_then(Value::as_str).filter(|p| hex::decode_array::<32>(p).is_some()).ok_or_else(bad)?;
    let token = ep.get("token").and_then(Value::as_str).filter(|t| vault_proto::b64::decode(t).is_some_and(|b| b.len() == 32)).ok_or_else(bad)?;
    let port = ep.get("port").and_then(Value::as_u64).filter(|p| (1..=65535).contains(p)).ok_or_else(bad)?;
    let hints: Vec<String> = ep.get("host_hints").and_then(Value::as_array).ok_or_else(bad)?.iter().map(|h| h.as_str().map(String::from)).collect::<Option<_>>().ok_or_else(bad)?;
    if hints.len() > 8 || hints.iter().any(|h| h.parse::<std::net::IpAddr>().is_err()) {
        return Err(bad());
    }
    Ok(json!({ "spki_sha256": pin, "token": token, "port": port, "host_hints": hints }))
}

/// Read back the pairing's `peer_endpoint` (the F.2c peer client).
pub fn stored_peer_endpoint() -> Result<Option<Value>, ErrorCode> {
    crate::keychain::read_item(PEER_ENDPOINT_ITEM)?.map(|b| serde_json::from_slice(&b).map_err(|_| ErrorCode::DbCorrupt)).transpose()
}

