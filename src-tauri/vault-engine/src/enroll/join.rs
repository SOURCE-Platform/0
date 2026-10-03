//! The new device's side of enrollment (spec §5.1–§5.2, v0.5 §22.10): the
//! SOURCE Vault phone joining a vault its Mac authorizes. The engine never
//! touches the network — Swift carries these frames over TLS pinned to
//! the QR's `fp` — but every value that enters the transcript, and the SAS
//! the user compares, is computed here.

use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use super::transcript::{self, Binding};
use super::wire::{self, HelloReply};
use crate::crypto::hex;
use crate::crypto::secret::random_secret;
use crate::device::identity::SeDevice;
use crate::errors::ErrorCode;
use crate::registry::device::{DeviceIdentity, PLATFORM_IOS};
use crate::sync::session::Transfer;

/// What the Mac's QR encodes (§5.2; main app `EnrollmentPayloadV2`).
#[derive(Debug, Clone, Deserialize)]
pub struct Qr {
    pub v: u8,
    pub host: String,
    pub port: u16,
    pub fp: String,
    pub secret: String,
    pub mac_device_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinStage {
    /// The hello was built; waiting for the Mac's reply.
    AwaitingReply,
    /// The SAS is on screen; the bundle may stream in.
    AwaitingBundle,
}

/// One join attempt. Its device identity lives in the vault directory and
/// is wiped if the attempt does not finish (§5.2 "nothing is persisted on
/// the new device before envelope decapsulation succeeds").
pub struct JoinSession {
    pub fp: [u8; 32],
    pub secret: zeroize::Zeroizing<[u8; 16]>,
    pub mac_device_id: [u8; 16],
    pub nonce_n: [u8; 16],
    pub stage: JoinStage,
    pub reply: Option<HelloReply>,
    pub transfer: Option<Transfer>,
    pub started: Instant,
}

impl JoinSession {
    /// The §5.2 whole-flow limit.
    pub fn expired(&self) -> bool {
        self.started.elapsed() > super::SESSION_TTL
    }

    pub fn reply_ids(&self) -> Result<([u8; 16], [u8; 16], [u8; 16]), ErrorCode> {
        let r = self.reply.as_ref().ok_or(ErrorCode::BadState)?;
        let id = |s: &str| hex::decode_array::<16>(s).ok_or(ErrorCode::ProtocolViolation);
        Ok((id(&r.new_device_id)?, id(&r.nonce_e)?, id(&r.vault_id)?))
    }
}

/// Parse the QR and mint the hello (§5.2 ENROLL_HELLO) with this device's
/// fresh Secure Enclave keys (a Face-ID-bound agreement key, §22.4).
pub fn begin(qr: &Qr, me: &SeDevice) -> Result<(JoinSession, Value), ErrorCode> {
    if qr.v != wire::PROTO_V2 {
        return Err(ErrorCode::ProtocolViolation);
    }
    let fp = hex::decode_array::<32>(&qr.fp).ok_or(ErrorCode::InvalidInput)?;
    let mac_device_id = hex::decode_array::<16>(&qr.mac_device_id).ok_or(ErrorCode::InvalidInput)?;
    let secret = transcript::decode_secret(&qr.secret).ok_or(ErrorCode::InvalidInput)?;
    let mut nonce_n = [0u8; 16];
    nonce_n.copy_from_slice(&random_secret().expose()[..16]);
    let hello = json!({
        "proto": wire::PROTO_V2,
        "secret": qr.secret,
        "nonce_n": hex::encode(nonce_n),
        "sign_pub": hex::encode(me.sign_pub()),
        "agree_pub": hex::encode(me.agree_pub()),
        "name": me.device_name(),
        "platform": PLATFORM_IOS,
    });
    let session = JoinSession {
        fp,
        secret: zeroize::Zeroizing::new(secret),
        mac_device_id,
        nonce_n,
        stage: JoinStage::AwaitingReply,
        reply: None,
        transfer: None,
        started: Instant::now(),
    };
    Ok((session, hello))
}

/// The Mac's hello reply: it must name the Mac the QR named; the SAS is
/// then computed here from the same bytes the Mac used, never received.
pub fn hello_reply(s: &mut JoinSession, me: &SeDevice, reply: HelloReply) -> Result<String, ErrorCode> {
    if s.stage != JoinStage::AwaitingReply || s.expired() {
        return Err(ErrorCode::BadState);
    }
    if hex::decode_array::<16>(&reply.mac_device_id) != Some(s.mac_device_id) {
        return Err(ErrorCode::ProtocolViolation);
    }
    s.reply = Some(reply);
    let (new_id, nonce_e, _) = s.reply_ids()?;
    let t = transcript::transcript(&Binding {
        fp: &s.fp,
        secret: &s.secret,
        nonce_e: &nonce_e,
        nonce_n: &s.nonce_n,
        mac_device_id: &s.mac_device_id,
        new_device_id: &new_id,
        sign_pub: &me.sign_pub(),
        agree_pub: &me.agree_pub(),
    });
    s.stage = JoinStage::AwaitingBundle;
    transcript::sas(&t)
}
