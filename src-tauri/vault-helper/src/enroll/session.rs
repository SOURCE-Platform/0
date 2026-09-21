//! The enrollment session: one at a time, single-use secret, 300 s TTL,
//! torn down after 5 failed secret presentations (§5.2, §5.3).

use std::time::{Duration, Instant};

use subtle::ConstantTimeEq;

use crate::crypto::registry::RegistryEntry;
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;

/// §5.2: the whole flow lives 300 s.
pub const SESSION_TTL: Duration = Duration::from_secs(300);
/// §5.2: five wrong secrets end the session.
pub const MAX_FAILURES: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// QR shown; waiting for the phone's ENROLL_HELLO.
    AwaitingHello,
    /// Transcript fixed and SAS displayed; waiting for the user to
    /// confirm on the Mac (and for the LA presence check).
    AwaitingConfirm,
    /// Bundle handed to the phone; waiting for ENROLL_ACK.
    AwaitingAck,
}

/// The new device's public identity, as presented in ENROLL_HELLO and
/// bound by the transcript.
#[derive(Clone)]
pub struct Peer {
    pub device_id: [u8; 16],
    pub device_name: String,
    pub platform: u8,
    pub sign_pub: [u8; 65],
    pub agree_pub: [u8; 65],
}

pub struct EnrollSession {
    /// SHA-256 of the ephemeral server's certificate DER (QR `fp`).
    pub fp: [u8; 32],
    secret: [u8; 16],
    pub nonce_e: [u8; 16],
    pub started: Instant,
    pub failures: u32,
    pub stage: Stage,
    pub peer: Option<Peer>,
    pub transcript: Option<[u8; 32]>,
    pub sas: Option<String>,
    /// Built at `confirm`, appended to the registry only at `ack`.
    pub entry: Option<RegistryEntry>,
    /// The per-device backup credential issued to this device (§11.4),
    /// held until the ACK lands so it can be recorded for re-enveloping.
    pub cred: Option<SecretBytes<32>>,
}

impl EnrollSession {
    pub fn new(fp: [u8; 32]) -> EnrollSession {
        let mut secret = [0u8; 16];
        let mut nonce_e = [0u8; 16];
        getrandom::fill(&mut secret).expect("OS CSPRNG");
        getrandom::fill(&mut nonce_e).expect("OS CSPRNG");
        EnrollSession {
            fp,
            secret,
            nonce_e,
            started: Instant::now(),
            failures: 0,
            stage: Stage::AwaitingHello,
            peer: None,
            transcript: None,
            sas: None,
            entry: None,
            cred: None,
        }
    }

    pub fn secret(&self) -> &[u8; 16] {
        &self.secret
    }

    pub fn expired(&self) -> bool {
        self.started.elapsed() >= SESSION_TTL
    }

    pub fn expires_in_secs(&self) -> u64 {
        SESSION_TTL
            .saturating_sub(self.started.elapsed())
            .as_secs()
    }

    /// Constant-time secret check. A wrong secret counts against
    /// `MAX_FAILURES`; the caller tears the session down when this
    /// returns `Err` with the counter exhausted.
    pub fn verify_secret(&mut self, presented: &[u8]) -> Result<(), ErrorCode> {
        if self.expired() {
            return Err(ErrorCode::PanelCancelled);
        }
        let ok: bool = presented.len() == self.secret.len()
            && self.secret.ct_eq(presented).into();
        if ok {
            Ok(())
        } else {
            self.failures += 1;
            Err(ErrorCode::WrongCredential)
        }
    }

    pub fn out_of_attempts(&self) -> bool {
        self.failures >= MAX_FAILURES
    }
}

impl Drop for EnrollSession {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.secret.zeroize();
        self.nonce_e.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_secret_counts_down_to_teardown() {
        let mut s = EnrollSession::new([1u8; 32]);
        for i in 1..=MAX_FAILURES {
            assert_eq!(s.verify_secret(b"not-the-secret-xx"), Err(ErrorCode::WrongCredential));
            assert_eq!(s.failures, i);
        }
        assert!(s.out_of_attempts());
    }

    #[test]
    fn correct_secret_verifies_and_length_is_checked() {
        let mut s = EnrollSession::new([1u8; 32]);
        let secret = *s.secret();
        assert!(s.verify_secret(&secret).is_ok());
        assert!(s.verify_secret(&secret[..15]).is_err());
        assert_eq!(s.failures, 1);
    }
}
