//! Peer sync envelopes (spec v0.5 §22.8; wire annex revision 3 A.1):
//! `PeerRequest` and `PeerResponse`, their prehashes and strict decoding.
//!
//! ```text
//! request  sig = ECDSA-P256(SHA-256("ov0/peer/request/v1"  ‖ tlv)), r‖s, low-S
//! response sig = ECDSA-P256(SHA-256("ov0/peer/response/v1" ‖ tlv)), r‖s, low-S
//! ```
//!
//! Both are single §4.2 Entries (no Document wrapper), like
//! `ProviderRequest`. Decoding is strict: every field present exactly
//! once, exact widths, no empty value, no unknown tag, and re-encoding
//! reproduces the input. The bodies live in `peer::body`.

pub mod body;
pub mod exchange;

use sha2::{Digest, Sha256};

use crate::crypto::ecdsa;
use crate::crypto::tlv::{EntryBuilder, EntryReader};
use crate::errors::ErrorCode;

pub const PROTO: u32 = 1;
const REQUEST_PREFIX: &[u8] = b"ov0/peer/request/v1";
const RESPONSE_PREFIX: &[u8] = b"ov0/peer/response/v1";

/// §22.8 operation table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PeerOp {
    Hello,
    State,
    Heads,
    RevsGet,
    RevsPut,
    Status,
}

impl PeerOp {
    pub fn code(self) -> u16 {
        match self {
            PeerOp::Hello => 1,
            PeerOp::State => 2,
            PeerOp::Heads => 3,
            PeerOp::RevsGet => 4,
            PeerOp::RevsPut => 5,
            PeerOp::Status => 6,
        }
    }

    pub fn from_code(c: u64) -> Option<PeerOp> {
        Some(match c {
            1 => PeerOp::Hello,
            2 => PeerOp::State,
            3 => PeerOp::Heads,
            4 => PeerOp::RevsGet,
            5 => PeerOp::RevsPut,
            6 => PeerOp::Status,
            _ => return None,
        })
    }
}

/// §22.8 response status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerStatus {
    Ok,
    BadState,
    Limit,
    NothingNewer,
    FormatInvalid,
}

impl PeerStatus {
    pub fn code(self) -> u16 {
        match self {
            PeerStatus::Ok => 0,
            PeerStatus::BadState => 1,
            PeerStatus::Limit => 2,
            PeerStatus::NothingNewer => 3,
            PeerStatus::FormatInvalid => 4,
        }
    }

    pub fn from_code(c: u64) -> Option<PeerStatus> {
        Some(match c {
            0 => PeerStatus::Ok,
            1 => PeerStatus::BadState,
            2 => PeerStatus::Limit,
            3 => PeerStatus::NothingNewer,
            4 => PeerStatus::FormatInvalid,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRequest {
    pub vault_id: [u8; 16],
    pub sender_device_id: [u8; 16],
    pub receiver_device_id: [u8; 16],
    pub operation: PeerOp,
    pub body_sha256: [u8; 32],
    pub t: u64,
    pub n: [u8; 16],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerResponse {
    pub vault_id: [u8; 16],
    pub responder_device_id: [u8; 16],
    pub requester_device_id: [u8; 16],
    pub request_prehash: [u8; 32],
    pub status: PeerStatus,
    pub body_sha256: [u8; 32],
    pub t: u64,
}

fn bad() -> ErrorCode {
    ErrorCode::PeerAuthInvalid
}

/// Strict entry: exactly `tags`, in order, none empty.
fn strict<'a>(bytes: &'a [u8], tags: &[u8]) -> Result<EntryReader<'a>, ErrorCode> {
    let r = EntryReader::parse(bytes).map_err(|_| bad())?;
    let present: Vec<u8> = r.tags().collect();
    if present != tags || tags.iter().any(|t| r.get(*t).is_none_or(|v| v.is_empty())) {
        return Err(bad());
    }
    Ok(r)
}

fn fixed<const N: usize>(r: &EntryReader<'_>, t: u8) -> Result<[u8; N], ErrorCode> {
    r.get(t).ok_or_else(bad)?.try_into().map_err(|_| bad())
}

fn uint(r: &EntryReader<'_>, t: u8) -> Result<u64, ErrorCode> {
    r.get_uint(t).map_err(|_| bad())?.ok_or_else(bad)
}

impl PeerRequest {
    pub fn encode(&self) -> Vec<u8> {
        EntryBuilder::new()
            .field_uint(0x01, u64::from(PROTO))
            .and_then(|b| b.field_bytes(0x02, &self.vault_id))
            .and_then(|b| b.field_bytes(0x03, &self.sender_device_id))
            .and_then(|b| b.field_bytes(0x04, &self.receiver_device_id))
            .and_then(|b| b.field_uint(0x05, u64::from(self.operation.code())))
            .and_then(|b| b.field_bytes(0x06, &self.body_sha256))
            .and_then(|b| b.field_uint(0x07, self.t))
            .and_then(|b| b.field_bytes(0x08, &self.n))
            .expect("ascending tags")
            .build()
    }

    pub fn prehash(&self) -> [u8; 32] {
        prehash(REQUEST_PREFIX, &self.encode())
    }

    pub fn decode(bytes: &[u8]) -> Result<PeerRequest, ErrorCode> {
        let r = strict(bytes, &[1, 2, 3, 4, 5, 6, 7, 8])?;
        if uint(&r, 0x01)? != u64::from(PROTO) {
            return Err(bad());
        }
        let req = PeerRequest {
            vault_id: fixed(&r, 0x02)?,
            sender_device_id: fixed(&r, 0x03)?,
            receiver_device_id: fixed(&r, 0x04)?,
            operation: PeerOp::from_code(uint(&r, 0x05)?).ok_or_else(bad)?,
            body_sha256: fixed(&r, 0x06)?,
            t: uint(&r, 0x07)?,
            n: fixed(&r, 0x08)?,
        };
        if req.encode() != bytes {
            return Err(bad());
        }
        Ok(req)
    }
}

impl PeerResponse {
    pub fn encode(&self) -> Vec<u8> {
        EntryBuilder::new()
            .field_uint(0x01, u64::from(PROTO))
            .and_then(|b| b.field_bytes(0x02, &self.vault_id))
            .and_then(|b| b.field_bytes(0x03, &self.responder_device_id))
            .and_then(|b| b.field_bytes(0x04, &self.requester_device_id))
            .and_then(|b| b.field_bytes(0x05, &self.request_prehash))
            .and_then(|b| b.field_uint(0x06, u64::from(self.status.code())))
            .and_then(|b| b.field_bytes(0x07, &self.body_sha256))
            .and_then(|b| b.field_uint(0x08, self.t))
            .expect("ascending tags")
            .build()
    }

    pub fn prehash(&self) -> [u8; 32] {
        prehash(RESPONSE_PREFIX, &self.encode())
    }

    pub fn decode(bytes: &[u8]) -> Result<PeerResponse, ErrorCode> {
        let r = strict(bytes, &[1, 2, 3, 4, 5, 6, 7, 8])?;
        if uint(&r, 0x01)? != u64::from(PROTO) {
            return Err(bad());
        }
        let resp = PeerResponse {
            vault_id: fixed(&r, 0x02)?,
            responder_device_id: fixed(&r, 0x03)?,
            requester_device_id: fixed(&r, 0x04)?,
            request_prehash: fixed(&r, 0x05)?,
            status: PeerStatus::from_code(uint(&r, 0x06)?).ok_or_else(bad)?,
            body_sha256: fixed(&r, 0x07)?,
            t: uint(&r, 0x08)?,
        };
        if resp.encode() != bytes {
            return Err(bad());
        }
        Ok(resp)
    }
}

fn prehash(prefix: &[u8], tlv: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(prefix);
    h.update(tlv);
    h.finalize().into()
}

/// Verify a peer signature over a prehash under a resolved public key.
pub fn verify(prehash: &[u8; 32], sig: &[u8], signer_pub: &[u8; 65]) -> Result<(), ErrorCode> {
    ecdsa::verify_prehash(signer_pub, prehash, sig).map_err(|_| bad())
}

pub fn body_hash(body: &[u8]) -> [u8; 32] {
    Sha256::digest(body).into()
}
