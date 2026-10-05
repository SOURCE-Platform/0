//! Enrollment transcript, SAS, and the ENROLL_ACK digest (§5.2).
//!
//! The transcript binds everything the user is about to approve: the TLS
//! certificate the phone pinned off the Mac's screen, the single-use
//! secret, both nonces, both device ids, and both of the new device's
//! public keys. The SAS the two screens display is a function of exactly
//! those bytes, so a matching SAS means the two devices agree on all of
//! them — a man in the middle would have to have matched every field.

use sha2::{Digest, Sha256};

use crate::crypto::hkdf;
use crate::errors::ErrorCode;

/// v2 (owner decision 2026-10-03, review SEC-B3): binds the authorizing
/// helper's own signing key, so a main process that terminates the
/// channel cannot present a matching SAS with its own keys.
pub const TRANSCRIPT_PREFIX: &[u8] = b"ov0/enroll/transcript/v2";
pub const ACK_PREFIX: &[u8] = b"ov0/enroll/ack/v1";

/// The repo's unambiguous 32-character alphabet (no 0/1/I/O), used both
/// for the SAS and for the QR secret's base32 encoding (§5.2).
pub const ALPHABET: &[u8; 32] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";

pub const SAS_LEN: usize = 8;

/// Everything the transcript binds. All fields are public values except
/// `secret`, which both sides already hold.
pub struct Binding<'a> {
    /// SHA-256 of the enrollment server's certificate DER (QR `fp`).
    pub fp: &'a [u8; 32],
    /// SHA-256 of the authorizing Mac's SE signing public key (QR
    /// `mac_key`) — the helper's own identity, not main's.
    pub mac_key: &'a [u8; 32],
    pub secret: &'a [u8; 16],
    pub nonce_e: &'a [u8; 16],
    pub nonce_n: &'a [u8; 16],
    pub mac_device_id: &'a [u8; 16],
    pub new_device_id: &'a [u8; 16],
    pub sign_pub: &'a [u8; 65],
    pub agree_pub: &'a [u8; 65],
}

pub fn transcript(b: &Binding<'_>) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(TRANSCRIPT_PREFIX);
    h.update(b.fp);
    h.update(b.mac_key);
    h.update(b.secret);
    h.update(b.nonce_e);
    h.update(b.nonce_n);
    h.update(b.mac_device_id);
    h.update(b.new_device_id);
    h.update(b.sign_pub);
    h.update(b.agree_pub);
    h.finalize().into()
}

/// 8 characters, 40 bits, from HKDF over the transcript (§5.2).
pub fn sas(transcript: &[u8; 32]) -> Result<String, ErrorCode> {
    let okm = hkdf::hkdf32(transcript, &[], hkdf::INFO_ENROLL_SAS)
        .map_err(|_| ErrorCode::Internal)?;
    Ok(base32_chars(&okm.expose()[..5], SAS_LEN))
}

/// The QR secret back to its 16 bytes (the repo's base32 alphabet);
/// anything else is simply a wrong secret.
pub fn decode_secret(encoded: &str) -> Option<[u8; 16]> {
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut out = Vec::with_capacity(16);
    for ch in encoded.bytes() {
        let idx = ALPHABET.iter().position(|&c| c == ch)? as u32;
        acc = (acc << 5) | idx;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    out.truncate(16);
    out.try_into().ok()
}

pub const PROOF_PREFIX: &[u8] = b"ov0/enroll/proof/v1";
pub const COMMIT_PREFIX: &[u8] = b"ov0/enroll/commit/v1";

/// The QR's `commit` (review SEC-B1, 0f5f21b): the helper fixes `nonce_e`
/// and the new device's id before any hello, so neither side's code can be
/// searched for — a main process relaying the pairing gets one 2^-40 guess
/// per session, not a choice of replies.
pub fn commitment(nonce_e: &[u8; 16], new_device_id: &[u8; 16]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(COMMIT_PREFIX);
    h.update(nonce_e);
    h.update(new_device_id);
    h.finalize().into()
}

/// The enrollment server's bundle and ACK routes answer only the device
/// that sent the hello: HMAC-SHA256 keyed by the QR secret over the
/// hello's nonce (review SEC-I3). A host that merely found the port has
/// neither.
pub fn route_proof(secret: &[u8; 16], nonce_n: &[u8; 16]) -> [u8; 32] {
    use hmac::{Hmac, KeyInit, Mac};
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret).expect("any key length");
    mac.update(PROOF_PREFIX);
    mac.update(nonce_n);
    mac.finalize().into_bytes().into()
}

/// `mac_key`: SHA-256 of a device's 65-byte signing public key.
pub fn key_fingerprint(sign_pub: &[u8; 65]) -> [u8; 32] {
    Sha256::digest(sign_pub).into()
}

/// What the new device signs to prove its SE signing key exists (§5.2).
pub fn ack_digest(registry_head: &[u8; 32], mac_device_id: &[u8; 16]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(ACK_PREFIX);
    h.update(registry_head);
    h.update(mac_device_id);
    h.finalize().into()
}

/// Base32 (repo alphabet, no padding) over `bytes`, truncated to `chars`
/// characters. 16 bytes → 26 characters; 5 bytes → 8 characters.
pub fn base32_chars(bytes: &[u8], chars: usize) -> String {
    let mut out = String::with_capacity(chars);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &b in bytes {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 && out.len() < chars {
            bits -= 5;
            out.push(ALPHABET[((acc >> bits) & 0x1f) as usize] as char);
        }
    }
    // Trailing bits (16 bytes is not a multiple of 5 bits) are left-padded
    // with zeros, the usual base32 tail rule.
    if out.len() < chars && bits > 0 {
        out.push(ALPHABET[((acc << (5 - bits)) & 0x1f) as usize] as char);
    }
    out
}

/// Encode a 16-byte secret for the QR payload: 26 characters, no padding.
pub fn encode_secret(secret: &[u8; 16]) -> String {
    base32_chars(secret, 26)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding<'a>(sign: &'a [u8; 65], agree: &'a [u8; 65]) -> Binding<'a> {
        static FP: [u8; 32] = [1u8; 32];
        static KEY: [u8; 32] = [12u8; 32];
        static SECRET: [u8; 16] = [2u8; 16];
        static NE: [u8; 16] = [3u8; 16];
        static NN: [u8; 16] = [4u8; 16];
        static MAC: [u8; 16] = [5u8; 16];
        static NEW: [u8; 16] = [6u8; 16];
        Binding {
            fp: &FP,
            mac_key: &KEY,
            secret: &SECRET,
            nonce_e: &NE,
            nonce_n: &NN,
            mac_device_id: &MAC,
            new_device_id: &NEW,
            sign_pub: sign,
            agree_pub: agree,
        }
    }

    #[test]
    fn sas_is_eight_chars_from_the_alphabet() {
        let s = sas(&[7u8; 32]).unwrap();
        assert_eq!(s.len(), SAS_LEN);
        assert!(s.bytes().all(|c| ALPHABET.contains(&c)));
    }

    #[test]
    fn every_bound_field_changes_the_sas() {
        let sign = [8u8; 65];
        let agree = [9u8; 65];
        let base = sas(&transcript(&binding(&sign, &agree))).unwrap();
        let other_sign = [10u8; 65];
        assert_ne!(base, sas(&transcript(&binding(&other_sign, &agree))).unwrap());
        let other_agree = [11u8; 65];
        assert_ne!(base, sas(&transcript(&binding(&sign, &other_agree))).unwrap());
        let mut b = binding(&sign, &agree);
        let flipped = [99u8; 16];
        b.nonce_n = &flipped;
        assert_ne!(base, sas(&transcript(&b)).unwrap());
        // v2: another helper key, another code (review SEC-B3).
        let mut b = binding(&sign, &agree);
        let other_key = [13u8; 32];
        b.mac_key = &other_key;
        assert_ne!(base, sas(&transcript(&b)).unwrap());
    }

    #[test]
    fn the_route_proof_needs_the_secret_and_the_nonce() {
        let p = route_proof(&[1; 16], &[2; 16]);
        assert_ne!(p, route_proof(&[3; 16], &[2; 16]));
        assert_ne!(p, route_proof(&[1; 16], &[4; 16]));
    }

    #[test]
    fn secret_encoding_is_26_unambiguous_chars() {
        let s = encode_secret(&[0xABu8; 16]);
        assert_eq!(s.len(), 26);
        assert!(s.bytes().all(|c| ALPHABET.contains(&c)));
        assert_ne!(s, encode_secret(&[0xACu8; 16]));
    }
}
