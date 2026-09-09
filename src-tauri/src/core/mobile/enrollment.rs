use rand::distr::{Alphanumeric, SampleString};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const SECRET_TTL: Duration = Duration::from_secs(300);
const SECRET_LEN: usize = 32;

/// What the QR code on the Mac's screen encodes.
///
/// The phone learns the certificate fingerprint here — off the screen, over the
/// camera — *before* it opens a connection. That is the whole point: an attacker
/// on the network cannot alter what is printed on your monitor, so the phone can
/// refuse any certificate that does not match before it sends anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentPayload {
    pub v: u8,
    pub host: String,
    pub port: u16,
    pub fp: String,
    pub secret: String,
    pub name: String,
}

/// A single-use secret proving the phone physically saw the Mac's screen.
#[derive(Default)]
pub struct Enrollment {
    current: Mutex<Option<PendingSecret>>,
}

struct PendingSecret {
    secret: String,
    expires_at: Instant,
}

impl Enrollment {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint a fresh secret, invalidating any previous one.
    pub async fn issue(&self) -> String {
        let secret = Alphanumeric.sample_string(&mut rand::rng(), SECRET_LEN);
        *self.current.lock().await = Some(PendingSecret {
            secret: secret.clone(),
            expires_at: Instant::now() + SECRET_TTL,
        });
        secret
    }

    /// Consume a secret. Returns false for wrong, expired, or already-used ones.
    pub async fn redeem(&self, candidate: &str) -> bool {
        let mut current = self.current.lock().await;
        let Some(pending) = current.as_ref() else {
            return false;
        };
        if pending.expires_at <= Instant::now() {
            *current = None;
            return false;
        }
        // Constant-time-ish compare: same length check plus full-length XOR.
        let expected = pending.secret.as_bytes();
        let given = candidate.as_bytes();
        let matched = expected.len() == given.len()
            && expected
                .iter()
                .zip(given.iter())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0;
        if matched {
            *current = None;
        }
        matched
    }

    pub async fn clear(&self) {
        *self.current.lock().await = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn secret_is_single_use() {
        let enrollment = Enrollment::new();
        let secret = enrollment.issue().await;
        assert_eq!(secret.len(), SECRET_LEN);
        assert!(enrollment.redeem(&secret).await);
        assert!(!enrollment.redeem(&secret).await, "replay must fail");
    }

    #[tokio::test]
    async fn wrong_secret_is_rejected_without_consuming() {
        let enrollment = Enrollment::new();
        let secret = enrollment.issue().await;
        assert!(!enrollment.redeem("nope").await);
        assert!(!enrollment.redeem(&secret[..SECRET_LEN - 1]).await);
        // The real secret still works — a bad guess must not burn it.
        assert!(enrollment.redeem(&secret).await);
    }

    #[tokio::test]
    async fn issuing_again_invalidates_the_previous_secret() {
        let enrollment = Enrollment::new();
        let first = enrollment.issue().await;
        let second = enrollment.issue().await;
        assert_ne!(first, second);
        assert!(!enrollment.redeem(&first).await);
        assert!(enrollment.redeem(&second).await);
    }

    #[tokio::test]
    async fn nothing_to_redeem_before_a_qr_is_shown() {
        let enrollment = Enrollment::new();
        assert!(!enrollment.redeem("anything").await);
    }
}
