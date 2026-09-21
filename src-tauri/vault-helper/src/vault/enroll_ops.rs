//! Enrollment ops (§5): `begin_enrollment`, the two relayed frames, the
//! Mac-side confirmation, and the ACK that finally writes the registry
//! entry.
//!
//! The helper never touches the network. The main process carries opaque
//! frames between its ephemeral TLS server and this socket; every
//! decision — secret verification, transcript, SAS, signing, envelope
//! sealing, registry append — happens here, with the VK resident.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::{ev_state, lock_core, Deps, OpOutcome, VaultCore};
use crate::crypto::{ecdsa, hex};
use crate::device::identity::SeDevice;
use crate::enroll::session::{EnrollSession, Peer, Stage};
use crate::enroll::{transcript, wire};
use crate::errors::ErrorCode;
use crate::registry::chain::EpochPolicy;
use crate::registry::device::{random_uuid, DeviceIdentity, PLATFORM_IOS, PLATFORM_MACOS};
use crate::state::VaultState;

/// Registries here carry no recovery epochs to authorize: enrollment
/// runs on an unlocked, live vault whose chain this device wrote.
pub(super) const POLICY: EpochPolicy<'static> = EpochPolicy::CheckpointAnchored;

const MAX_NAME_CHARS: usize = 64;

pub(super) fn require_unlocked(core: &Arc<Mutex<VaultCore>>) -> Result<(), ErrorCode> {
    if lock_core(core).state == VaultState::Unlocked {
        Ok(())
    } else {
        Err(ErrorCode::BadState)
    }
}

/// §5.1 step 1. The main process has already bound its ephemeral TLS
/// listener and passes the certificate fingerprint it will serve; the
/// fingerprint enters the transcript, so the channel the phone pinned off
/// the screen is what the SAS confirms.
pub fn begin_enrollment(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    if let Err(e) = require_unlocked(core) {
        return OpOutcome::err(e);
    }
    let Some(fp) = frame.get("fp").and_then(Value::as_str).and_then(hex::decode_array::<32>) else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    let (dir, vault_id) = {
        let c = lock_core(core);
        let Some(h) = c.header.as_ref() else {
            return OpOutcome::err(ErrorCode::BadState);
        };
        (c.vault_dir.clone(), h.vault_id.0)
    };
    let me = match SeDevice::load(&dir) {
        Ok(d) => d,
        Err(e) => return OpOutcome::err(e),
    };
    let session = EnrollSession::new(fp);
    let response = json!({
        "secret": transcript::encode_secret(session.secret()),
        "mac_device_id": hex::encode(me.device_id()),
        "vault_id": hex::encode(vault_id),
        "expires_in": session.expires_in_secs(),
    });
    lock_core(core).enroll = Some(session);
    OpOutcome::ok(response)
}

pub fn cancel_enrollment(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    lock_core(core).enroll = None; // secret zeroizes on drop
    OpOutcome::ok(json!({}))
}

/// §5.2 ENROLL_HELLO. Authenticates the phone's first frame, fixes the
/// transcript, and returns what the phone needs to derive the same SAS
/// for itself — the SAS is never sent, so a match means both sides
/// computed it from the same bytes.
pub fn enroll_hello(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    if let Err(e) = require_unlocked(core) {
        return OpOutcome::err(e);
    }
    let hello: wire::Hello = match serde_json::from_value(frame.clone()) {
        Ok(h) => h,
        Err(_) => return OpOutcome::err(ErrorCode::InvalidInput),
    };
    if hello.proto != wire::PROTO_V2 {
        return OpOutcome::err(ErrorCode::ProtocolViolation);
    }
    let (mut secret, fp, nonce_e) = {
        let mut c = lock_core(core);
        let Some(session) = c.enroll.as_mut() else {
            return OpOutcome::err(ErrorCode::BadState);
        };
        if session.stage != Stage::AwaitingHello || session.expired() {
            c.enroll = None;
            return OpOutcome::err(ErrorCode::BadState);
        }
        let presented = decode_secret(&hello.secret);
        let result = session.verify_secret(presented.as_deref().unwrap_or(&[]));
        if result.is_err() {
            // §5.3: five wrong secrets end the session for good.
            let done = session.out_of_attempts();
            if done {
                c.enroll = None;
            }
            return OpOutcome::err(ErrorCode::WrongCredential);
        }
        (*session.secret(), session.fp, session.nonce_e)
    };

    let peer = match parse_peer(&hello) {
        Ok(p) => p,
        Err(e) => return OpOutcome::err(e),
    };
    let (dir, vault_id) = {
        let c = lock_core(core);
        let Some(h) = c.header.as_ref() else {
            return OpOutcome::err(ErrorCode::BadState);
        };
        (c.vault_dir.clone(), h.vault_id.0)
    };
    let me = match SeDevice::load(&dir) {
        Ok(d) => d,
        Err(e) => return OpOutcome::err(e),
    };
    let t = transcript::transcript(&transcript::Binding {
        fp: &fp,
        secret: &secret,
        nonce_e: &nonce_e,
        nonce_n: &peer_nonce(&hello).unwrap_or([0u8; 16]),
        mac_device_id: &me.device_id(),
        new_device_id: &peer.device_id,
        sign_pub: &peer.sign_pub,
        agree_pub: &peer.agree_pub,
    });
    let sas = match transcript::sas(&t) {
        Ok(s) => s,
        Err(e) => return OpOutcome::err(e),
    };
    // The op's copy of the secret is wiped here; the session keeps the
    // only live one until it is torn down.
    {
        use zeroize::Zeroize;
        secret.zeroize();
    }
    let reply = wire::HelloReply {
        new_device_id: hex::encode(peer.device_id),
        nonce_e: hex::encode(nonce_e),
        mac_device_id: hex::encode(me.device_id()),
        vault_id: hex::encode(vault_id),
    };
    let mut c = lock_core(core);
    let Some(session) = c.enroll.as_mut() else {
        return OpOutcome::err(ErrorCode::BadState);
    };
    session.peer = Some(peer);
    session.transcript = Some(t);
    session.sas = Some(sas.clone());
    session.stage = Stage::AwaitingConfirm;
    OpOutcome::ok(json!({
        "reply": serde_json::to_value(&reply).unwrap_or(Value::Null),
        "sas": sas,
    }))
}

fn peer_nonce(hello: &wire::Hello) -> Option<[u8; 16]> {
    hex::decode_array::<16>(&hello.nonce_n)
}

fn decode_secret(encoded: &str) -> Option<Vec<u8>> {
    // The QR carries the secret in the repo's base32 alphabet; anything
    // else is simply a wrong secret.
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut out = Vec::with_capacity(16);
    for ch in encoded.bytes() {
        let idx = transcript::ALPHABET.iter().position(|&c| c == ch)? as u32;
        acc = (acc << 5) | idx;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    out.truncate(16);
    (out.len() == 16).then_some(out)
}

fn parse_peer(hello: &wire::Hello) -> Result<Peer, ErrorCode> {
    let sign_pub = hex::decode_array::<65>(&hello.sign_pub).ok_or(ErrorCode::InvalidInput)?;
    let agree_pub = hex::decode_array::<65>(&hello.agree_pub).ok_or(ErrorCode::InvalidInput)?;
    // §4.4 rule 8: both keys must be on-curve uncompressed points.
    ecdsa::parse_verifying_key(&sign_pub).map_err(|_| ErrorCode::InvalidInput)?;
    ecdsa::parse_verifying_key(&agree_pub).map_err(|_| ErrorCode::InvalidInput)?;
    if sign_pub == agree_pub {
        // §2.7: never one key for both roles.
        return Err(ErrorCode::InvalidInput);
    }
    if hello.platform != PLATFORM_IOS && hello.platform != PLATFORM_MACOS {
        return Err(ErrorCode::InvalidInput);
    }
    if hello.name.chars().count() > MAX_NAME_CHARS || hello.name.is_empty() {
        return Err(ErrorCode::InvalidInput);
    }
    if peer_nonce(hello).is_none() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(Peer {
        // The Mac assigns the registry identity: a new device cannot
        // choose its own device_id (or collide with an enrolled one).
        device_id: random_uuid(),
        device_name: hello.name.clone(),
        platform: hello.platform,
        sign_pub,
        agree_pub,
    })
}

/// §5.1: the user has compared the SAS on both screens. Behind an LA
/// presence check, this signs the enroll entry and seals the envelope —
/// but writes nothing to the registry until the ACK proves the phone
/// holds the private key it presented.
pub fn enroll_confirm(core: &Arc<Mutex<VaultCore>>, deps: &Deps) -> OpOutcome {
    if let Err(e) = require_unlocked(core) {
        return OpOutcome::err(e);
    }
    {
        let c = lock_core(core);
        match c.enroll.as_ref() {
            Some(s) if s.stage == Stage::AwaitingConfirm && !s.expired() => {}
            _ => return OpOutcome::err(ErrorCode::BadState),
        }
    }
    {
        let mut c = lock_core(core);
        c.state = VaultState::Authorizing;
    }
    deps.events.emit(ev_state(VaultState::Authorizing));
    let allowed = deps.la.check("Source Vault: add this device");
    {
        let mut c = lock_core(core);
        if c.state == VaultState::Authorizing {
            c.state = VaultState::Unlocked;
            deps.events.emit(ev_state(VaultState::Unlocked));
        }
    }
    if !allowed {
        return OpOutcome::err(ErrorCode::PresenceDenied);
    }
    // A passed presence check restarts the auto-lock window, like every
    // other presence-gated op (§1.6).
    lock_core(core).note_authorization();
    match super::enroll_commit::build_bundle(core) {
        Ok(bundle) => OpOutcome::ok(json!({"bundle": bundle})),
        Err(e) => OpOutcome::err(e),
    }
}
