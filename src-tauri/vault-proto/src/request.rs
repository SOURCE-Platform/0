//! `ProviderRequest` (spec v0.4 §11.4): the canonical TLV every provider
//! request is signed over, its prehash, and the `Ov0-Auth` header.
//!
//! ```text
//! sig = ECDSA-P256(SHA-256("ov0/provider/request/v2" ‖ tlv)), 64 B r‖s, low-S
//! Ov0-Auth: v2.<base64url(tlv)>.<base64url(sig)>
//! ```
//!
//! 0x08 `signer_device_id` is present iff the signer class is device; 0x0B
//! `expected_state` iff the operation is `state_commit`. Decoding is strict
//! (canonical TLV, exact widths, re-encoding reproduces the input).

use sha2::{Digest, Sha256};

use crate::b64;
use crate::crypto::ecdsa;
use crate::crypto::hex;
use crate::crypto::recovery_auth::{CLASS_DEVICE, CLASS_MP, CLASS_RK};
use crate::crypto::tlv::{EntryBuilder, EntryReader};
use crate::errors::ErrorCode;

pub const PROTO: u32 = 2;
const PREFIX: &[u8] = b"ov0/provider/request/v2";
/// Longest accepted `audience`/`method`/`path` (bytes).
const MAX_TEXT: usize = 256;

mod tag {
    pub const PROTO: u8 = 0x01;
    pub const AUDIENCE: u8 = 0x02;
    pub const VAULT_ID: u8 = 0x03;
    pub const OPERATION: u8 = 0x04;
    pub const METHOD: u8 = 0x05;
    pub const PATH: u8 = 0x06;
    pub const SIGNER_CLASS: u8 = 0x07;
    pub const SIGNER_DEVICE_ID: u8 = 0x08;
    pub const SIGNER_KEY_ID: u8 = 0x09;
    pub const BODY_SHA256: u8 = 0x0A;
    pub const EXPECTED_STATE: u8 = 0x0B;
    pub const T: u8 = 0x0C;
    pub const N: u8 = 0x0D;
}

/// §11.4 policy table operations (32–47 are reserved for Phase G push).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    StateGet,
    BlobGet,
    BlobPut,
    StateCommit,
}

impl Operation {
    pub fn code(self) -> u16 {
        match self {
            Operation::StateGet => 1,
            Operation::BlobGet => 2,
            Operation::BlobPut => 3,
            Operation::StateCommit => 4,
        }
    }

    pub fn from_code(c: u64) -> Option<Operation> {
        match c {
            1 => Some(Operation::StateGet),
            2 => Some(Operation::BlobGet),
            3 => Some(Operation::BlobPut),
            4 => Some(Operation::StateCommit),
            _ => None,
        }
    }

    pub fn is_mutating(self) -> bool {
        matches!(self, Operation::BlobPut | Operation::StateCommit)
    }

    /// The canonical method and path for this operation (§11.3 routes).
    /// `blob` is required for the blob operations and refused otherwise.
    pub fn route(self, vault_id: &[u8; 16], blob: Option<&[u8; 32]>) -> Option<(&'static str, String)> {
        let vid = hex::encode(vault_id);
        match (self, blob) {
            (Operation::StateGet, None) => Some(("GET", format!("/v2/vaults/{vid}/state"))),
            (Operation::StateCommit, None) => Some(("POST", format!("/v2/vaults/{vid}/state"))),
            (Operation::BlobGet, Some(b)) => Some(("GET", format!("/v2/vaults/{vid}/blobs/{}", hex::encode(b)))),
            (Operation::BlobPut, Some(b)) => Some(("PUT", format!("/v2/vaults/{vid}/blobs/{}", hex::encode(b)))),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRequest {
    pub audience: String,
    pub vault_id: [u8; 16],
    pub operation: Operation,
    pub method: String,
    pub path: String,
    pub signer_class: u8,
    pub signer_device_id: Option<[u8; 16]>,
    pub signer_key_id: [u8; 32],
    pub body_sha256: [u8; 32],
    pub expected_state: Option<[u8; 32]>,
    pub t: u64,
    pub n: [u8; 16],
}

/// Who signs a request (§11.4): a device's SE key, or a recovery class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignerId {
    Device { device_id: [u8; 16], key_id: [u8; 32] },
    Recovery { class: u8, key_id: [u8; 32] },
}

impl ProviderRequest {
    /// Build the canonical request for `operation` — the helper supplies
    /// every field itself (§11.4 "Helper checks before signing").
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        audience: &str,
        vault_id: [u8; 16],
        operation: Operation,
        blob: Option<&[u8; 32]>,
        signer: SignerId,
        body: &[u8],
        expected_state: Option<[u8; 32]>,
        t: u64,
        n: [u8; 16],
    ) -> Result<ProviderRequest, ErrorCode> {
        let (method, path) = operation.route(&vault_id, blob).ok_or(ErrorCode::SigningRefused)?;
        if expected_state.is_some() != (operation == Operation::StateCommit) || !valid_audience(audience) {
            return Err(ErrorCode::SigningRefused);
        }
        let (signer_class, signer_device_id, signer_key_id) = match signer {
            SignerId::Device { device_id, key_id } => (CLASS_DEVICE, Some(device_id), key_id),
            SignerId::Recovery { class, key_id } if class == CLASS_MP || class == CLASS_RK => (class, None, key_id),
            SignerId::Recovery { .. } => return Err(ErrorCode::SigningRefused),
        };
        Ok(ProviderRequest {
            audience: audience.to_string(),
            vault_id,
            operation,
            method: method.to_string(),
            path,
            signer_class,
            signer_device_id,
            signer_key_id,
            body_sha256: body_hash(body),
            expected_state,
            t,
            n,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = EntryBuilder::new()
            .field_uint(tag::PROTO, u64::from(PROTO))
            .and_then(|b| b.field_bytes(tag::AUDIENCE, self.audience.as_bytes()))
            .and_then(|b| b.field_bytes(tag::VAULT_ID, &self.vault_id))
            .and_then(|b| b.field_uint(tag::OPERATION, u64::from(self.operation.code())))
            .and_then(|b| b.field_bytes(tag::METHOD, self.method.as_bytes()))
            .and_then(|b| b.field_bytes(tag::PATH, self.path.as_bytes()))
            .and_then(|b| b.field_uint(tag::SIGNER_CLASS, u64::from(self.signer_class)))
            .expect("ascending tags");
        if let Some(d) = &self.signer_device_id {
            b = b.field_bytes(tag::SIGNER_DEVICE_ID, d).expect("ascending");
        }
        b = b
            .field_bytes(tag::SIGNER_KEY_ID, &self.signer_key_id)
            .and_then(|b| b.field_bytes(tag::BODY_SHA256, &self.body_sha256))
            .expect("ascending");
        if let Some(e) = &self.expected_state {
            b = b.field_bytes(tag::EXPECTED_STATE, e).expect("ascending");
        }
        b.field_uint(tag::T, self.t)
            .and_then(|b| b.field_bytes(tag::N, &self.n))
            .expect("ascending")
            .build()
    }

    /// SHA-256("ov0/provider/request/v2" ‖ tlv) — the signed prehash.
    pub fn prehash(&self) -> [u8; 32] {
        prehash_tlv(&self.encode())
    }

    /// Strict decode plus the presence rules; ASCII-only text fields.
    pub fn decode(bytes: &[u8]) -> Result<ProviderRequest, ErrorCode> {
        let bad = || ErrorCode::AuthInvalid;
        let r = EntryReader::parse(bytes).map_err(|_| bad())?;
        if r.get_uint(tag::PROTO).map_err(|_| bad())? != Some(u64::from(PROTO)) {
            return Err(bad());
        }
        let text = |t: u8| -> Result<String, ErrorCode> {
            let v = r.get(t).ok_or_else(bad)?;
            if v.is_empty() || v.len() > MAX_TEXT || !v.iter().all(|c| (0x21..0x7f).contains(c)) {
                return Err(bad());
            }
            String::from_utf8(v.to_vec()).map_err(|_| bad())
        };
        let fixed = |t: u8| r.get(t).ok_or_else(bad);
        let uint = |t: u8| r.get_uint(t).map_err(|_| bad())?.ok_or_else(bad);
        let operation = Operation::from_code(uint(tag::OPERATION)?).ok_or_else(bad)?;
        let signer_class = u8::try_from(uint(tag::SIGNER_CLASS)?).map_err(|_| bad())?;
        let req = ProviderRequest {
            audience: text(tag::AUDIENCE)?,
            vault_id: fixed(tag::VAULT_ID)?.try_into().map_err(|_| bad())?,
            operation,
            method: text(tag::METHOD)?,
            path: text(tag::PATH)?,
            signer_class,
            signer_device_id: r.get(tag::SIGNER_DEVICE_ID).map(|d| d.try_into()).transpose().map_err(|_| bad())?,
            signer_key_id: fixed(tag::SIGNER_KEY_ID)?.try_into().map_err(|_| bad())?,
            body_sha256: fixed(tag::BODY_SHA256)?.try_into().map_err(|_| bad())?,
            expected_state: r.get(tag::EXPECTED_STATE).map(|d| d.try_into()).transpose().map_err(|_| bad())?,
            t: uint(tag::T)?,
            n: fixed(tag::N)?.try_into().map_err(|_| bad())?,
        };
        let device_ok = match signer_class {
            CLASS_DEVICE => req.signer_device_id.is_some(),
            CLASS_MP | CLASS_RK => req.signer_device_id.is_none(),
            _ => false,
        };
        let state_ok = req.expected_state.is_some() == (operation == Operation::StateCommit);
        if !device_ok || !state_ok || !valid_audience(&req.audience) || req.encode() != bytes {
            return Err(bad());
        }
        Ok(req)
    }
}

pub fn prehash_tlv(tlv: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(PREFIX);
    h.update(tlv);
    h.finalize().into()
}

pub fn body_hash(body: &[u8]) -> [u8; 32] {
    Sha256::digest(body).into()
}

/// `Ov0-Auth: v2.<base64url(tlv)>.<base64url(sig)>`.
pub fn auth_header(tlv: &[u8], sig: &[u8; 64]) -> String {
    format!("v2.{}.{}", b64::encode(tlv), b64::encode(sig))
}

/// Parse the header into (request, raw tlv, signature). Signature
/// verification is the caller's (it must first resolve the signer).
pub fn parse_auth_header(value: &str) -> Result<(ProviderRequest, Vec<u8>, [u8; 64]), ErrorCode> {
    let bad = || ErrorCode::AuthInvalid;
    let mut parts = value.split('.');
    let (Some("v2"), Some(tlv), Some(sig), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(bad());
    };
    let tlv = b64::decode(tlv).ok_or_else(bad)?;
    let sig: [u8; 64] = b64::decode(sig).ok_or_else(bad)?.try_into().map_err(|_| bad())?;
    let req = ProviderRequest::decode(&tlv)?;
    Ok((req, tlv, sig))
}

/// Verify a request signature under a resolved signer public key.
pub fn verify_signature(tlv: &[u8], sig: &[u8; 64], signer_pub: &[u8; 65]) -> Result<(), ErrorCode> {
    ecdsa::verify_prehash(signer_pub, &prehash_tlv(tlv), sig).map_err(|_| ErrorCode::AuthInvalid)
}

/// Provider origin syntax (§11.4): lowercase `https://host[:port]`, no
/// path, query or userinfo. Plain `http://` is accepted only for loopback
/// hosts (test transports); which origins a helper will sign for is its
/// own compiled-in allowlist, not this check.
pub fn valid_audience(a: &str) -> bool {
    let (host_port, loopback_only) = if let Some(r) = a.strip_prefix("https://") {
        (r, false)
    } else if let Some(r) = a.strip_prefix("http://") {
        (r, true)
    } else {
        return false;
    };
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (host_port, None),
    };
    let host_ok = !host.is_empty()
        && host.len() <= 253
        && host.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'.' || c == b'-');
    let port_ok = port.is_none_or(|p| !p.is_empty() && p.len() <= 5 && !p.starts_with('0') && p.bytes().all(|c| c.is_ascii_digit()) && p.parse::<u32>().is_ok_and(|n| n <= 65535));
    host_ok && port_ok && (!loopback_only || host == "127.0.0.1" || host == "localhost")
}
