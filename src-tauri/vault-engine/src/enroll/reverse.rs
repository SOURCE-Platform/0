//! Reverse enrollment (spec v0.5 §22.13; design `phase-f2e-design.md`
//! revision 2): the iPhone authorizes a replacement Mac. The Mac (joiner)
//! serves TLS and shows a `v:3` QR carrying its commitment; the iPhone
//! (authorizer) assigns the Mac's `device_id`, signs, seals and publishes.
//! Every string here is domain-separated from forward enrollment (`v:2`).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::transcript::{decode_secret, key_fingerprint};
use crate::crypto::hex;
use crate::errors::ErrorCode;

pub const PROTO_V3: u8 = 3;
pub const TRANSCRIPT_PREFIX: &[u8] = b"ov0/renroll/transcript/v1";
pub const COMMIT_PREFIX: &[u8] = b"ov0/renroll/commit/v1";
pub const PROOF_PREFIX: &[u8] = b"ov0/renroll/proof/v1";

/// The QR on the joining Mac's screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Qr {
    pub v: u8,
    pub host: String,
    pub port: u16,
    pub fp: String,
    pub secret: String,
    pub commit: String,
}

/// iPhone → Mac (`POST /v1/vault/renroll/hello`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub v: u8,
    pub secret: String,
    pub nonce_e: String,
    pub authorizer_device_id: String,
    pub authorizer_sign_pub: String,
    pub new_device_id: String,
    pub vault_id: String,
}

/// Mac → iPhone: opens the QR commitment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub nonce_n: String,
    pub sign_pub: String,
    pub agree_pub: String,
    pub name: String,
    pub platform: u8,
}

/// Everything the transcript binds.
pub struct Binding<'a> {
    pub fp: &'a [u8; 32],
    pub authorizer_key: &'a [u8; 32],
    pub secret: &'a [u8; 16],
    pub nonce_e: &'a [u8; 16],
    pub nonce_n: &'a [u8; 16],
    pub authorizer_device_id: &'a [u8; 16],
    pub new_device_id: &'a [u8; 16],
    pub sign_pub: &'a [u8; 65],
    pub agree_pub: &'a [u8; 65],
    pub vault_id: &'a [u8; 16],
}

pub fn transcript(b: &Binding<'_>) -> [u8; 32] {
    let mut h = Sha256::new();
    for part in [
        TRANSCRIPT_PREFIX,
        b.fp.as_slice(),
        b.authorizer_key.as_slice(),
        b.secret.as_slice(),
        b.nonce_e.as_slice(),
        b.nonce_n.as_slice(),
        b.authorizer_device_id.as_slice(),
        b.new_device_id.as_slice(),
        b.sign_pub.as_slice(),
        b.agree_pub.as_slice(),
        b.vault_id.as_slice(),
    ] {
        h.update(part);
    }
    h.finalize().into()
}

/// The joiner's half, fixed before the authorizer reveals anything; it
/// also covers the name and platform the Mac will be enrolled with.
pub fn commitment(nonce_n: &[u8; 16], sign_pub: &[u8; 65], agree_pub: &[u8; 65], name: &str, platform: u8) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(COMMIT_PREFIX);
    h.update(nonce_n);
    h.update(sign_pub);
    h.update(agree_pub);
    h.update((name.len() as u32).to_be_bytes());
    h.update(name.as_bytes());
    h.update([platform]);
    h.finalize().into()
}

/// The bundle and `done` routes answer only the iPhone that sent the hello.
pub fn route_proof(secret: &[u8; 16], nonce_e: &[u8; 16]) -> [u8; 32] {
    use hmac::{Hmac, KeyInit, Mac};
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret).expect("any key length");
    mac.update(PROOF_PREFIX);
    mac.update(nonce_e);
    mac.finalize().into_bytes().into()
}

/// A host the iPhone accepts in a `v:3` QR: an IP literal on a private,
/// link-local or loopback network — the Mac must be next to the user
/// (design §8, review SPEC-I8).
pub fn local_host(host: &str) -> bool {
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(a)) => a.is_private() || a.is_link_local() || a.is_loopback(),
        Ok(std::net::IpAddr::V6(a)) => a.is_loopback() || (a.segments()[0] & 0xfe00) == 0xfc00 || (a.segments()[0] & 0xffc0) == 0xfe80,
        Err(_) => false,
    }
}

/// The QR, parsed: version, local host, 32-byte fp and commit, secret.
pub struct ParsedQr {
    pub fp: [u8; 32],
    pub secret: [u8; 16],
    pub commit: [u8; 32],
}

pub fn parse_qr(qr: &Qr) -> Result<ParsedQr, ErrorCode> {
    if qr.v != PROTO_V3 {
        return Err(ErrorCode::ProtocolViolation);
    }
    if !local_host(&qr.host) || qr.port == 0 {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(ParsedQr {
        fp: hex::decode_array::<32>(&qr.fp).ok_or(ErrorCode::InvalidInput)?,
        secret: decode_secret(&qr.secret).ok_or(ErrorCode::InvalidInput)?,
        commit: hex::decode_array::<32>(&qr.commit).ok_or(ErrorCode::InvalidInput)?,
    })
}

pub fn authorizer_key(sign_pub: &[u8; 65]) -> [u8; 32] {
    key_fingerprint(sign_pub)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_commitment_covers_every_field() {
        let base = commitment(&[1; 16], &[2; 65], &[3; 65], "Mac", 1);
        assert_ne!(base, commitment(&[9; 16], &[2; 65], &[3; 65], "Mac", 1));
        assert_ne!(base, commitment(&[1; 16], &[9; 65], &[3; 65], "Mac", 1));
        assert_ne!(base, commitment(&[1; 16], &[2; 65], &[9; 65], "Mac", 1));
        assert_ne!(base, commitment(&[1; 16], &[2; 65], &[3; 65], "Mac2", 1));
        assert_ne!(base, commitment(&[1; 16], &[2; 65], &[3; 65], "Mac", 2));
    }

    #[test]
    fn only_local_hosts_are_accepted() {
        for ok in ["192.168.1.5", "10.0.0.2", "172.16.4.4", "169.254.1.1", "127.0.0.1", "fe80::1", "fd00::5"] {
            assert!(local_host(ok), "{ok}");
        }
        for bad in ["8.8.8.8", "example.test", "2001:db8::1", ""] {
            assert!(!local_host(bad), "{bad}");
        }
    }

    #[test]
    fn the_transcript_is_not_the_forward_one() {
        assert_ne!(TRANSCRIPT_PREFIX, super::super::transcript::TRANSCRIPT_PREFIX);
        assert_ne!(PROOF_PREFIX, super::super::transcript::PROOF_PREFIX);
        assert_ne!(COMMIT_PREFIX, super::super::transcript::COMMIT_PREFIX);
    }
}
