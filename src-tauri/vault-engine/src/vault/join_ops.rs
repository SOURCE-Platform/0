//! The phone's enrollment ops (spec §5, v0.5 §22.10; FFI catalogue §4):
//! SOURCE Vault joins a vault its Mac authorizes. Swift carries each frame
//! over TLS pinned to the QR's `fp`; every decision is here.
//!
//! - `join_begin {qr, name}` → this device's Secure Enclave keys and the
//!   hello to send, plus where to send it;
//! - `join_hello {reply}` → the SAS to compare with the Mac's screen;
//! - `join_bundle_begin {sha256, size}` → a session the bundle streams into
//!   (stream ops), once the user confirmed on both screens;
//! - `join_complete {session}` → the §22.10 checks, the vault written,
//!   and the ENROLL_ACK signature to send; the vault stays LOCKED;
//! - `join_finish` once the Mac accepted the ACK; `join_abort` otherwise,
//!   which removes everything the attempt created (§5.2 "Cancellation").

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
use crate::storage::{kv, VaultStore};
use crate::sync::materialize::{materialize, Anchor};
use crate::sync::session::Transfer;

/// Set while the Mac has not yet accepted this device's ACK.
const UNACKED_KEY: &str = "join_unacked";
/// `peer_endpoint` without its token (§22.8, wire annex A.4).
pub const PEER_ENDPOINT_KEY: &str = "peer_endpoint";
/// The peer token, in the Keychain only (`WhenUnlockedThisDeviceOnly`).
pub const PEER_TOKEN_ITEM: &str = "com.racker.zero.vault.peer-token";
/// The largest bundle accepted (hex-encoded objects of a large vault).
pub const MAX_BUNDLE: u64 = 256 << 20;
const REASON: &str = "SOURCE Vault: finish pairing with your Mac";

fn run(f: impl FnOnce() -> Result<Value, ErrorCode>) -> OpOutcome {
    f().map_or_else(OpOutcome::err, OpOutcome::ok)
}

fn uninitialized(c: &VaultCore) -> Result<(), ErrorCode> {
    if c.state == VaultState::Uninitialized { Ok(()) } else { Err(ErrorCode::BadState) }
}

pub fn join_begin(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let qr: Qr = serde_json::from_value(frame.get("qr").cloned().ok_or(ErrorCode::InvalidInput)?).map_err(|_| ErrorCode::InvalidInput)?;
        let name = frame.get("name").and_then(Value::as_str).unwrap_or("iPhone").chars().take(64).collect::<String>();
        let dir = {
            let c = lock_core(core);
            uninitialized(&c)?;
            c.vault_dir.clone()
        };
        // Fresh keys every attempt (a replaced identity's are deleted).
        let me = SeDevice::create(&dir, &name, PLATFORM_IOS)?;
        let (session, hello) = join::begin(&qr, &me)?;
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
        me.adopt_id(&dir, new_id)?;
        Ok(json!({ "sas": sas }))
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
        // Taken out under the lock; the Face ID prompt runs without it.
        let (session, dir) = {
            let mut c = lock_core(core);
            uninitialized(&c)?;
            let s = c.provider.join.take().filter(|s| s.transfer.as_ref().is_some_and(|t| t.id == id)).ok_or(ErrorCode::TransferInvalid)?;
            (s, c.vault_dir.clone())
        };
        let bytes = session.transfer.as_ref().and_then(|t| t.received.values().next().cloned()).ok_or(ErrorCode::TransferInvalid)?;
        let raw: Value = serde_json::from_slice(&bytes).map_err(|_| ErrorCode::FormatInvalid)?;
        let bundle: wire::Bundle = serde_json::from_value(raw.clone()).map_err(|_| ErrorCode::FormatInvalid)?;
        let me = SeDevice::load(&dir)?;
        let (_, nonce_e, vault_id) = session.reply_ids()?;
        let anchor = Anchor { me: &me, mac_device_id: session.mac_device_id, vault_id, nonce_e };
        let tag = me.key_tag().to_string();
        let open = move |f: &crate::device::envelope::DeviceEnvelopeFile| crate::device::envelope::open_envelope_for(&tag, REASON, &vault_id, f);
        let (store, vk, head) = match materialize(&dir, &bundle, &anchor, &open) {
            Ok(done) => done,
            Err(e) => {
                discard(&dir);
                return Err(e);
            }
        };
        // A key verified through §22.10 is one this device trusts: its SE
        // commitment is signed now, as at vault creation (§22.4).
        if let Err(e) = super::vk_commit::record(&dir, &vault_id, store.header.vk_generation, &vk) {
            discard(&dir);
            return Err(e);
        }
        drop(vk);
        keep_peer_endpoint(&store, raw.get("peer_endpoint"))?;
        kv::put(&store.conn, UNACKED_KEY, &true)?;
        let _ = super::floor::reset(&store);
        let sig = me.sign_prehash(&transcript::ack_digest(&head, &session.mac_device_id)).map_err(|_| ErrorCode::DeviceNotAuthorized)?;
        let mut c = lock_core(core);
        c.header = Some(store.header.clone());
        c.state = VaultState::Locked; // the VK above is dropped (zeroized) here
        Ok(json!({ "ack": { "signature": hex::encode(sig) }, "registry_head": hex::encode(head) }))
    })
}

/// The Mac accepted the ACK: the vault is this device's for good.
pub fn join_finish(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    run(|| {
        let dir = lock_core(core).vault_dir.clone();
        let store = VaultStore::open(&dir)?;
        kv::get::<bool>(&store.conn, UNACKED_KEY)?.ok_or(ErrorCode::BadState)?;
        kv::delete(&store.conn, UNACKED_KEY)?;
        Ok(json!({}))
    })
}

/// Pairing failed or was cancelled: nothing it created survives.
pub fn join_abort(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    run(|| {
        let mut c = lock_core(core);
        let dir = c.vault_dir.clone();
        let unacked = VaultStore::open(&dir).ok().is_some_and(|s| kv::get::<bool>(&s.conn, UNACKED_KEY).ok().flatten().is_some());
        if !(c.state == VaultState::Uninitialized || unacked) || c.vk.is_some() {
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

/// Remove an attempt's vault files and Secure Enclave keys.
fn discard(dir: &std::path::Path) {
    identity::wipe(dir);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let _ = if e.path().is_dir() { std::fs::remove_dir_all(e.path()) } else { std::fs::remove_file(e.path()) };
        }
    }
}

/// `peer_endpoint` (annex A.4): the pin, port and hints in the store, the
/// token in the Keychain only.
fn keep_peer_endpoint(store: &VaultStore, ep: Option<&Value>) -> Result<(), ErrorCode> {
    let Some(ep) = ep else { return Ok(()) };
    let token = ep.get("token").and_then(Value::as_str).ok_or(ErrorCode::FormatInvalid)?;
    crate::keychain::upsert_item(PEER_TOKEN_ITEM, token.as_bytes())?;
    let public = json!({ "spki_sha256": ep.get("spki_sha256"), "port": ep.get("port"), "host_hints": ep.get("host_hints") });
    kv::put(&store.conn, PEER_ENDPOINT_KEY, &public)
}
