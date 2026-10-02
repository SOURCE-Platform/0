//! Building and signing a `PeerResponse` (§22.8): the helper fills every
//! field itself, binding the request's prehash and the body's hash, so
//! nothing in the path (main included) can swap either.

use vault_proto::peer::{body_hash, PeerResponse, PeerStatus};

use super::verify::Accepted;
use super::{Ctx, Refusal};
use crate::registry::device::DeviceIdentity;

pub struct Signed {
    pub response_tlv: Vec<u8>,
    pub signature: [u8; 64],
    pub body: Vec<u8>,
}

pub fn sign(ctx: &Ctx, accepted: &Accepted, status: PeerStatus, body: Vec<u8>, now: u64) -> Result<Signed, Refusal> {
    let resp = PeerResponse {
        vault_id: ctx.vault_id,
        responder_device_id: ctx.me.device_id(),
        requester_device_id: accepted.req.sender_device_id,
        request_prehash: accepted.prehash,
        status,
        body_sha256: body_hash(&body),
        t: now,
    };
    // Cannot sign right now (Keychain / Secure Enclave unavailable, §1.6).
    let signature = ctx.me.sign_prehash(&resp.prehash()).map_err(|_| Refusal::Unavailable)?;
    Ok(Signed { response_tlv: resp.encode(), signature, body })
}

/// A non-zero status carries the empty body (A.1).
pub fn status_only(ctx: &Ctx, accepted: &Accepted, status: PeerStatus, now: u64) -> Result<Signed, Refusal> {
    sign(ctx, accepted, status, vault_proto::peer::body::empty(), now)
}
