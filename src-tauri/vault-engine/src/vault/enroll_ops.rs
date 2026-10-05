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
use crate::registry::device::{DeviceIdentity, PLATFORM_IOS, PLATFORM_MACOS};
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
    if !begin_allowed() {
        return OpOutcome::err(ErrorCode::BadState); // at most a few per minute
    }
    let session = EnrollSession::new(fp);
    let response = json!({
        "commit": hex::encode(session.commitment()),
        "secret": transcript::encode_secret(session.secret()),
        "mac_device_id": hex::encode(me.device_id()),
        // For the QR: the helper's own key, which the transcript binds.
        "mac_key": hex::encode(transcript::key_fingerprint(&me.sign_pub())),
        "vault_id": hex::encode(vault_id),
        "expires_in": session.expires_in_secs(),
    });
    lock_core(core).enroll = Some(session);
    OpOutcome::ok(response)
}

/// `enroll_proof {proof}`: main asks before serving the bundle or taking
/// an ACK — only the device that sent the hello can answer (review SEC-I3).
pub fn enroll_proof(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    let proof = frame.get("proof").and_then(Value::as_str).and_then(hex::decode_array::<32>).unwrap_or([0; 32]);
    let ok = lock_core(core).enroll.as_ref().is_some_and(|s| !s.expired() && s.verify_route_proof(&proof));
    if ok { OpOutcome::ok(json!({})) } else { OpOutcome::err(ErrorCode::WrongCredential) }
}

/// Pairing attempts per minute: each is one guess at a 40-bit code, so a
/// relay cannot churn sessions (review SEC-B1, 0f5f21b).
const BEGINS_PER_MINUTE: usize = 6;

fn begin_allowed() -> bool {
    use std::collections::VecDeque;
    use std::time::{Duration, Instant};
    static BEGUN: Mutex<VecDeque<Instant>> = Mutex::new(VecDeque::new());
    let mut q = BEGUN.lock().unwrap_or_else(|p| p.into_inner());
    while q.front().is_some_and(|t| t.elapsed() > Duration::from_secs(60)) {
        q.pop_front();
    }
    let mut limit = BEGINS_PER_MINUTE;
    // Debug builds: the test suites pair many synthetic phones per minute.
    #[cfg(debug_assertions)]
    if let Some(n) = std::env::var("OV0_VAULT_ENROLL_BEGINS_PER_MINUTE").ok().and_then(|v| v.parse().ok()) {
        limit = n;
    }
    if q.len() >= limit {
        return false;
    }
    q.push_back(Instant::now());
    true
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
    let (mut secret, fp, nonce_e, new_id) = {
        let mut c = lock_core(core);
        let Some(session) = c.enroll.as_mut() else {
            return OpOutcome::err(ErrorCode::BadState);
        };
        if session.stage != Stage::AwaitingHello || session.expired() {
            c.enroll = None;
            return OpOutcome::err(ErrorCode::BadState);
        }
        let presented = transcript::decode_secret(&hello.secret).map(|s| s.to_vec());
        let result = session.verify_secret(presented.as_deref().unwrap_or(&[]));
        if result.is_err() {
            // §5.3: five wrong secrets end the session for good.
            let done = session.out_of_attempts();
            if done {
                c.enroll = None;
            }
            return OpOutcome::err(ErrorCode::WrongCredential);
        }
        (*session.secret(), session.fp, session.nonce_e, session.new_device_id)
    };

    let peer = match parse_peer(&hello) {
        Ok(p) => Peer { device_id: new_id, ..p },
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
        mac_key: &transcript::key_fingerprint(&me.sign_pub()),
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
    session.nonce_n = peer_nonce(&hello);
    session.transcript = Some(t);
    session.sas = Some(sas.clone());
    session.stage = Stage::AwaitingConfirm;
    // The SAS goes to the helper's own panel at confirm, never to the main
    // process (owner decision 2026-10-03, review SEC-B3).
    OpOutcome::ok(json!({ "reply": serde_json::to_value(&reply).unwrap_or(Value::Null) }))
}

fn peer_nonce(hello: &wire::Hello) -> Option<[u8; 16]> {
    hex::decode_array::<16>(&hello.nonce_n)
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
        // The Mac assigns the registry identity (fixed at `begin` and
        // committed in the QR): a new device cannot choose its own.
        device_id: [0; 16],
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
        // Checked and entered under one lock: a lock landing in between
        // must not leave AUTHORIZING (then UNLOCKED) over no VK.
        let mut c = lock_core(core);
        match c.enroll.as_ref() {
            Some(s) if s.stage == Stage::AwaitingConfirm && !s.expired() && c.state == VaultState::Unlocked => {}
            _ => return OpOutcome::err(ErrorCode::BadState),
        }
        c.state = VaultState::Authorizing;
    }
    deps.events.emit(ev_state(VaultState::Authorizing));
    // §22.4 (F2-D3): presence and the current master password — a login
    // password alone never adds a device.
    let verdict = if deps.la.check("Source Vault: add this device") { prove_current_mp(core, deps) } else { Err(ErrorCode::PresenceDenied) };
    {
        let mut c = lock_core(core);
        if c.state == VaultState::Authorizing {
            c.state = VaultState::Unlocked;
            deps.events.emit(ev_state(VaultState::Unlocked));
        }
    }
    if let Err(e) = verdict {
        return OpOutcome::err(e);
    }
    // A passed presence check restarts the auto-lock window, like every
    // other presence-gated op (§1.6).
    lock_core(core).note_authorization();
    match super::enroll_commit::build_bundle(core) {
        Ok(bundle) => OpOutcome::ok(deliver_bundle(&mut lock_core(core), bundle)),
        Err(e) => OpOutcome::err(e),
    }
}

/// The helper panel shows the code the phone must show and collects the
/// MP; the MP must open the committed wrap.
fn prove_current_mp(core: &Arc<Mutex<VaultCore>>, deps: &Deps) -> Result<(), ErrorCode> {
    use super::setup::{emit_panel, PANEL_TIMEOUT_PUB};
    use super::{PanelOutcome, PanelRequest};
    let code = lock_core(core).enroll.as_ref().and_then(|s| s.sas.clone()).ok_or(ErrorCode::BadState)?;
    emit_panel(deps, true, PanelRequest::EnrollConfirm);
    let outcome = deps.panel.run_with_code(PanelRequest::EnrollConfirm, &code, PANEL_TIMEOUT_PUB);
    emit_panel(deps, false, PanelRequest::EnrollConfirm);
    let PanelOutcome::Submitted(mp) = outcome else {
        // Cancel in the Add Device panel is the Mac's "codes don't match":
        // the session ends and its secret is burned (§5.2, review VER-B1).
        lock_core(core).enroll = None;
        return Err(ErrorCode::PanelCancelled);
    };
    let mut c = lock_core(core);
    if c.state != VaultState::Authorizing {
        return Err(ErrorCode::BadState);
    }
    let proved = super::recovery_ops::prove_mp_resident(&c, &mp).map(|_| ());
    if proved == Err(ErrorCode::WrongCredential) {
        // §15 backoff, as every other master-password entry.
        let delay = c.record_failed_attempt();
        drop(c);
        std::thread::sleep(delay);
    }
    proved
}

/// Bundles up to this size travel inline in the `enroll_confirm` answer.
const INLINE_BUNDLE: usize = 32 * 1024;

/// §1.3 TR-09: a bundle too large for one frame becomes a one-blob
/// stream session (`{session, stream, size}`), read with `stream_read`
/// and closed with `session_close`.
pub fn deliver_bundle(c: &mut VaultCore, bundle: Value) -> Value {
    let bytes = serde_json::to_vec(&bundle).unwrap_or_default();
    if bytes.len() <= INLINE_BUNDLE {
        return json!({ "bundle": bundle });
    }
    let sha: [u8; 32] = <sha2::Sha256 as sha2::Digest>::digest(&bytes).into();
    let size = bytes.len();
    let t = crate::sync::session::Transfer::new([(sha, bytes)].into_iter().collect());
    let out = json!({ "session": hex::encode(t.id), "stream": hex::encode(sha), "size": size });
    c.provider.bundle = Some(t);
    out
}

