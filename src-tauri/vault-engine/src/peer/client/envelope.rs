//! The requester's envelopes (spec v0.5 §22.8): the phone builds and signs
//! every field of its `PeerRequest` itself — no caller digest is ever
//! signed — and checks each response in the requester order: canonical
//! parse → `request_prehash` = its one outstanding request → responder =
//! the device it addressed, active in its **committed** registry →
//! signature → body hash. Anything else is "unable to verify".

use vault_proto::crypto::tlv::{EntryBuilder, EntryReader};
use vault_proto::peer::{body_hash, status_body_ok, verify, PeerOp, PeerRequest, PeerResponse, PeerStatus};

use crate::errors::ErrorCode;
use crate::registry::chain::RegistryState;
use crate::registry::device::DeviceIdentity;

/// The one request in flight: what its response must answer.
pub struct Outstanding {
    pub prehash: [u8; 32],
    pub responder: [u8; 16],
    pub op: PeerOp,
}

/// Annex A.2.1 carriage: `{0x01 tlv, 0x02 signature, 0x03 body}`.
fn carriage(tlv: &[u8], sig: &[u8], body: &[u8]) -> Vec<u8> {
    EntryBuilder::new()
        .field_bytes(0x01, tlv)
        .and_then(|b| b.field_bytes(0x02, sig))
        .and_then(|b| b.field_bytes(0x03, body))
        .expect("ascending tags")
        .build()
}

/// A signed request to `responder` as its HTTP carriage entry.
pub fn request(me: &dyn DeviceIdentity, vault_id: [u8; 16], responder: [u8; 16], op: PeerOp, body: &[u8], now: u64) -> Result<(Vec<u8>, Outstanding), ErrorCode> {
    let mut n = [0u8; 16];
    getrandom::fill(&mut n).map_err(|_| ErrorCode::Internal)?;
    let req = PeerRequest {
        vault_id,
        sender_device_id: me.device_id(),
        receiver_device_id: responder,
        operation: op,
        body_sha256: body_hash(body),
        t: now,
        n,
    };
    let prehash = req.prehash();
    let sig = me.sign_prehash(&prehash).map_err(|_| ErrorCode::DeviceNotAuthorized)?;
    Ok((carriage(&req.encode(), &sig, body), Outstanding { prehash, responder, op }))
}

/// The response to `out`, checked in the requester order; its status and
/// body. Every failure is `PEER_AUTH_INVALID` ("unable to verify").
pub fn response(reg: &RegistryState, vault_id: [u8; 16], me: [u8; 16], out: &Outstanding, bytes: &[u8]) -> Result<(PeerStatus, Vec<u8>), ErrorCode> {
    let bad = ErrorCode::PeerAuthInvalid;
    let e = EntryReader::parse(bytes).map_err(|_| bad)?;
    if e.tags().collect::<Vec<_>>() != [1, 2, 3] {
        return Err(bad);
    }
    let (tlv, sig, body) = (e.get(1).ok_or(bad)?, e.get(2).ok_or(bad)?, e.get(3).ok_or(bad)?);
    let resp = PeerResponse::decode(tlv).map_err(|_| bad)?;
    // The responder check is defence in depth: the signature below is
    // verified under the addressed Mac's key whatever the field says.
    if resp.request_prehash != out.prehash || resp.vault_id != vault_id || resp.requester_device_id != me || resp.responder_device_id != out.responder {
        return Err(bad);
    }
    // A responder no longer active is "unable to verify" too (§22.8, PS-14).
    let signer = reg.active_device(&out.responder).ok_or(bad)?;
    verify(&resp.prehash(), sig, &signer.sign_pub).map_err(|_| bad)?;
    if body_hash(body) != resp.body_sha256 || !status_body_ok(resp.status, body) {
        return Err(bad);
    }
    Ok((resp.status, body.to_vec()))
}
