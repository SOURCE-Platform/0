//! The §6.5 signed iPhone approval (Phase G). Canonical TLV (§4.2), tags
//! ascending; a **challenge** is the same entry without the signature
//! (tag 0x10), sent by the Mac; the iPhone signs
//! `SHA-256("ov0/approval/sign/v1" ‖ challenge)` with its SE signing key.
//! Decoding is strict: re-encoding must reproduce the input.

use sha2::{Digest, Sha256};

use crate::crypto::tlv::{EntryBuilder, EntryReader};
use crate::errors::ErrorCode;

pub const PROTO: u64 = 1;
const SIGN_PREFIX: &[u8] = b"ov0/approval/sign/v1";
/// §6.6: a challenge lives 120 s.
pub const TTL_SECS: u64 = 120;
pub const MAX_ORIGIN: usize = 253;

/// §6.5 tag 0x06.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    FillPassword = 1,
    FillCard = 2,
    Reveal = 3,
    UnlockVault = 4,
    UpdatePassword = 5,
}

impl Action {
    pub fn from_code(c: u64) -> Option<Action> {
        Some(match c {
            1 => Action::FillPassword,
            2 => Action::FillCard,
            3 => Action::Reveal,
            4 => Action::UnlockVault,
            5 => Action::UpdatePassword,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Action::FillPassword => "fill_password",
            Action::FillCard => "fill_card",
            Action::Reveal => "reveal",
            Action::UnlockVault => "unlock_vault",
            Action::UpdatePassword => "update_password",
        }
    }
    pub fn from_name(n: &str) -> Option<Action> {
        [Action::FillPassword, Action::FillCard, Action::Reveal, Action::UnlockVault, Action::UpdatePassword].into_iter().find(|a| a.name() == n)
    }
    /// §6.5: a `credential_ref` for everything that acts on a credential.
    pub fn needs_ref(self) -> bool {
        self != Action::UnlockVault
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub request_id: [u8; 16],
    pub mac_device_id: [u8; 16],
    pub iphone_device_id: [u8; 16],
    pub origin: String,
    pub action: Action,
    pub credential_ref: Option<[u8; 16]>,
    pub nonce: [u8; 16],
    pub iat: u64,
    pub exp: u64,
}

fn bad() -> ErrorCode {
    ErrorCode::FormatInvalid
}

impl Challenge {
    fn builder(&self) -> Result<EntryBuilder, ErrorCode> {
        let mut b = EntryBuilder::new()
            .field_uint(0x01, PROTO)
            .and_then(|b| b.field_bytes(0x02, &self.request_id))
            .and_then(|b| b.field_bytes(0x03, &self.mac_device_id))
            .and_then(|b| b.field_bytes(0x04, &self.iphone_device_id))
            .and_then(|b| b.field_string(0x05, &self.origin))
            .and_then(|b| b.field_uint(0x06, self.action as u64))
            .map_err(|_| bad())?;
        if let Some(r) = &self.credential_ref {
            b = b.field_bytes(0x07, r).map_err(|_| bad())?;
        }
        b.field_bytes(0x08, &self.nonce)
            .and_then(|b| b.field_uint(0x09, self.iat))
            .and_then(|b| b.field_uint(0x0A, self.exp))
            .map_err(|_| bad())
    }

    pub fn encode(&self) -> Result<Vec<u8>, ErrorCode> {
        Ok(self.builder()?.build())
    }

    /// What the iPhone signs (§6.5).
    pub fn prehash(&self) -> Result<[u8; 32], ErrorCode> {
        let mut h = Sha256::new();
        h.update(SIGN_PREFIX);
        h.update(self.encode()?);
        Ok(h.finalize().into())
    }

    /// The signed approval: the challenge plus tag 0x10.
    pub fn signed(&self, signature: &[u8; 64]) -> Result<Vec<u8>, ErrorCode> {
        Ok(self.builder()?.field_bytes(0x10, signature).map_err(|_| bad())?.build())
    }

    fn read(e: &EntryReader<'_>) -> Result<Challenge, ErrorCode> {
        let arr16 = |t: u8| -> Result<[u8; 16], ErrorCode> { e.get(t).and_then(|v| <[u8; 16]>::try_from(v).ok()).ok_or_else(bad) };
        if e.get_uint(0x01).map_err(|_| bad())? != Some(PROTO) {
            return Err(bad());
        }
        let origin = e.get_string(0x05, MAX_ORIGIN).map_err(|_| bad())?.filter(|o| !o.is_empty()).ok_or_else(bad)?;
        let action = e.get_uint(0x06).map_err(|_| bad())?.and_then(Action::from_code).ok_or_else(bad)?;
        let credential_ref = match e.get(0x07) {
            Some(v) => Some(<[u8; 16]>::try_from(v).map_err(|_| bad())?),
            None => None,
        };
        if credential_ref.is_some() != action.needs_ref() {
            return Err(bad());
        }
        Ok(Challenge {
            request_id: arr16(0x02)?,
            mac_device_id: arr16(0x03)?,
            iphone_device_id: arr16(0x04)?,
            origin,
            action,
            credential_ref,
            nonce: arr16(0x08)?,
            iat: e.get_uint(0x09).map_err(|_| bad())?.ok_or_else(bad)?,
            exp: e.get_uint(0x0A).map_err(|_| bad())?.ok_or_else(bad)?,
        })
    }

    /// A challenge (no signature), strictly.
    pub fn decode(bytes: &[u8]) -> Result<Challenge, ErrorCode> {
        let e = EntryReader::parse(bytes).map_err(|_| bad())?;
        if e.get(0x10).is_some() {
            return Err(bad());
        }
        let c = Challenge::read(&e)?;
        if c.encode()? != bytes {
            return Err(bad());
        }
        Ok(c)
    }
}

/// A signed approval, strictly: the challenge it covers and the signature.
pub fn decode_signed(bytes: &[u8]) -> Result<(Challenge, [u8; 64]), ErrorCode> {
    let e = EntryReader::parse(bytes).map_err(|_| bad())?;
    let sig: [u8; 64] = e.get(0x10).and_then(|v| v.try_into().ok()).ok_or_else(bad)?;
    let c = Challenge::read(&e)?;
    if c.signed(&sig)? != bytes {
        return Err(bad());
    }
    Ok((c, sig))
}

/// A cancel from the phone: `{0x01 proto, 0x02 request_id, 0x11 cancelled=1}`
/// — unsigned by design (§6.6: no signature needed to cancel).
pub fn encode_cancel(request_id: &[u8; 16]) -> Vec<u8> {
    EntryBuilder::new()
        .field_uint(0x01, PROTO)
        .and_then(|b| b.field_bytes(0x02, request_id))
        .and_then(|b| b.field_uint(0x11, 1))
        .expect("ascending tags")
        .build()
}

pub fn decode_cancel(bytes: &[u8]) -> Option<[u8; 16]> {
    let e = EntryReader::parse(bytes).ok()?;
    let id: [u8; 16] = e.get(0x02)?.try_into().ok()?;
    (e.get_uint(0x11).ok()?? == 1 && encode_cancel(&id) == bytes).then_some(id)
}

/// `peer_approvals` answer: a §4.2 Document of challenges.
pub fn encode_list(challenges: &[Challenge]) -> Result<Vec<u8>, ErrorCode> {
    let entries = challenges.iter().map(Challenge::encode).collect::<Result<Vec<_>, _>>()?;
    Ok(crate::crypto::tlv::encode_document(&entries))
}

pub fn decode_list(bytes: &[u8]) -> Result<Vec<Challenge>, ErrorCode> {
    crate::crypto::tlv::decode_document(bytes).map_err(|_| bad())?.iter().map(|e| Challenge::decode(e)).collect()
}

/// `peer_approve` result codes (design §3).
pub mod result {
    pub const RELEASED: u8 = 0;
    pub const REPLAY: u8 = 1;
    pub const EXPIRED: u8 = 2;
    pub const MISMATCH: u8 = 3;
    pub const NOT_AUTHORIZED: u8 = 4;
    pub const CANCELLED: u8 = 5;
    pub const UNKNOWN: u8 = 6;
    /// DA-08: the signature does not verify (`SIGNATURE_INVALID`).
    pub const SIGNATURE: u8 = 7;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Challenge {
        Challenge { request_id: [1; 16], mac_device_id: [2; 16], iphone_device_id: [3; 16], origin: "https://example.test".into(), action: Action::FillPassword, credential_ref: Some([4; 16]), nonce: [5; 16], iat: 10, exp: 130 }
    }

    #[test]
    fn round_trips_strictly() {
        let c = sample();
        let bytes = c.encode().unwrap();
        assert_eq!(Challenge::decode(&bytes).unwrap(), c);
        let signed = c.signed(&[9; 64]).unwrap();
        assert_eq!(decode_signed(&signed).unwrap().0, c);
        assert!(Challenge::decode(&signed).is_err(), "a signed approval is not a challenge");
        assert_eq!(decode_list(&encode_list(&[c.clone(), c]).unwrap()).unwrap().len(), 2);
        assert_eq!(decode_cancel(&encode_cancel(&[7; 16])), Some([7; 16]));
    }

    #[test]
    fn the_ref_matches_the_action() {
        let mut c = sample();
        c.credential_ref = None;
        assert!(c.encode().and_then(|b| Challenge::decode(&b)).is_err());
        c.action = Action::UnlockVault;
        assert!(c.encode().and_then(|b| Challenge::decode(&b)).is_ok());
    }

    #[test]
    fn every_field_changes_the_prehash() {
        let base = sample().prehash().unwrap();
        let mut c = sample();
        c.origin = "https://evil.test".into();
        assert_ne!(base, c.prehash().unwrap());
        let mut c = sample();
        c.action = Action::Reveal;
        assert_ne!(base, c.prehash().unwrap());
    }
}
