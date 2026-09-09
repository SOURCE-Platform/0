use rand::distr::{Alphanumeric, SampleString};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const REQUEST_TTL: Duration = Duration::from_secs(180);
const MAX_PENDING: usize = 8;

/// Unambiguous alphabet for the comparison tag: no 0/O/1/I.
const SAS_ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
const SAS_LEN: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairState {
    Pending,
    Approved { token: String, device_id: String },
    Denied,
    Expired,
}

#[derive(Debug, Clone)]
pub struct PendingPair {
    pub request_id: String,
    pub device_id: String,
    pub device_name: String,
    pub expires_at: Instant,
    pub state: PairState,
}

/// Approval-based pairing: the phone asks, the Mac shows a dialog, the user
/// clicks Allow. No code is typed and no secret crosses the wire.
///
/// The security comes from the comparison tag (`short_auth_string`), which each
/// side derives from the TLS certificate *it actually sees*. An attacker
/// impersonating the Mac holds a different certificate, so the tag on the phone
/// will not match the tag in the Mac's dialog and the user aborts.
#[derive(Default)]
pub struct PairRequests {
    inner: Mutex<HashMap<String, PendingPair>>,
}

impl PairRequests {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a request and return its id. Returns `Err` when too many are queued.
    pub async fn open(&self, device_id: &str, device_name: &str) -> Result<String, String> {
        let mut pending = self.inner.lock().await;
        prune(&mut pending);
        if pending.len() >= MAX_PENDING {
            return Err("Too many pending pairing requests.".to_string());
        }
        // One live request per device — a retry replaces the previous attempt.
        pending.retain(|_, entry| entry.device_id != device_id);
        let request_id = uuid::Uuid::new_v4().to_string();
        pending.insert(
            request_id.clone(),
            PendingPair {
                request_id: request_id.clone(),
                device_id: device_id.to_string(),
                device_name: device_name.to_string(),
                expires_at: Instant::now() + REQUEST_TTL,
                state: PairState::Pending,
            },
        );
        Ok(request_id)
    }

    pub async fn list_pending(&self) -> Vec<PendingPair> {
        let mut pending = self.inner.lock().await;
        prune(&mut pending);
        pending
            .values()
            .filter(|entry| entry.state == PairState::Pending)
            .cloned()
            .collect()
    }

    /// Look up the device a request belongs to, so the caller can mint a token.
    pub async fn device_for(&self, request_id: &str) -> Option<(String, String)> {
        let mut pending = self.inner.lock().await;
        prune(&mut pending);
        pending
            .get(request_id)
            .filter(|entry| entry.state == PairState::Pending)
            .map(|entry| (entry.device_id.clone(), entry.device_name.clone()))
    }

    pub async fn approve(&self, request_id: &str, token: String, device_id: String) -> bool {
        let mut pending = self.inner.lock().await;
        prune(&mut pending);
        match pending.get_mut(request_id) {
            Some(entry) if entry.state == PairState::Pending => {
                entry.state = PairState::Approved { token, device_id };
                true
            }
            _ => false,
        }
    }

    pub async fn deny(&self, request_id: &str) -> bool {
        let mut pending = self.inner.lock().await;
        match pending.get_mut(request_id) {
            Some(entry) => {
                entry.state = PairState::Denied;
                true
            }
            None => false,
        }
    }

    /// Poll a request. An approved result is handed out exactly once.
    pub async fn poll(&self, request_id: &str) -> PairState {
        let mut pending = self.inner.lock().await;
        prune(&mut pending);
        let Some(entry) = pending.get(request_id) else {
            return PairState::Expired;
        };
        match entry.state.clone() {
            PairState::Approved { token, device_id } => {
                pending.remove(request_id);
                PairState::Approved { token, device_id }
            }
            PairState::Denied => {
                pending.remove(request_id);
                PairState::Denied
            }
            other => other,
        }
    }
}

fn prune(pending: &mut HashMap<String, PendingPair>) {
    let now = Instant::now();
    pending.retain(|_, entry| entry.expires_at > now);
}

pub fn new_token() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 48)
}

/// Derive the short comparison tag from a certificate fingerprint.
///
/// Both ends run this over the fingerprint of the certificate they see, so the
/// tags only agree when the phone is talking directly to this Mac.
pub fn short_auth_string(fingerprint: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"source-mobile-sas-v1");
    hasher.update(fingerprint.as_bytes());
    hasher
        .finalize()
        .iter()
        .take(SAS_LEN)
        .map(|byte| SAS_ALPHABET[*byte as usize % SAS_ALPHABET.len()] as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sas_is_stable_and_fingerprint_specific() {
        let tag = short_auth_string("aabbcc");
        assert_eq!(tag, short_auth_string("aabbcc"));
        assert_eq!(tag.len(), SAS_LEN);
        assert_ne!(tag, short_auth_string("aabbcd"));
    }

    #[test]
    fn sas_avoids_ambiguous_characters() {
        for seed in 0..256u32 {
            let tag = short_auth_string(&format!("fingerprint-{seed}"));
            assert!(!tag.contains(['0', 'O', '1', 'I']), "ambiguous char in {tag}");
        }
    }

    #[tokio::test]
    async fn approval_flow_hands_out_token_once() {
        let requests = PairRequests::new();
        let id = requests.open("device-a", "iPhone").await.expect("open");
        assert_eq!(requests.poll(&id).await, PairState::Pending);
        assert!(requests.approve(&id, "tok".into(), "device-a".into()).await);
        assert_eq!(
            requests.poll(&id).await,
            PairState::Approved {
                token: "tok".into(),
                device_id: "device-a".into()
            }
        );
        // Consumed — a replay gets nothing.
        assert_eq!(requests.poll(&id).await, PairState::Expired);
    }

    #[tokio::test]
    async fn denied_requests_never_yield_a_token() {
        let requests = PairRequests::new();
        let id = requests.open("device-b", "iPhone").await.expect("open");
        assert!(requests.deny(&id).await);
        assert_eq!(requests.poll(&id).await, PairState::Denied);
        assert!(!requests.approve(&id, "tok".into(), "device-b".into()).await);
    }

    #[tokio::test]
    async fn retry_replaces_the_previous_request_for_a_device() {
        let requests = PairRequests::new();
        let first = requests.open("device-c", "iPhone").await.expect("first");
        let second = requests.open("device-c", "iPhone").await.expect("second");
        assert_ne!(first, second);
        assert_eq!(requests.poll(&first).await, PairState::Expired);
        assert_eq!(requests.list_pending().await.len(), 1);
    }

    #[tokio::test]
    async fn unknown_request_is_expired() {
        let requests = PairRequests::new();
        assert_eq!(requests.poll("nope").await, PairState::Expired);
        assert!(requests.device_for("nope").await.is_none());
    }
}

#[cfg(test)]
mod cross_language_tests {
    use super::*;

    /// The phone derives this tag independently in Swift. If the two ever drift
    /// the camera-free pairing path silently breaks, so pin the exact values.
    #[test]
    fn matches_the_swift_implementation() {
        assert_eq!(short_auth_string("aabbcc"), "UAZY");
        assert_eq!(short_auth_string(&"a".repeat(64)), "XRU9");
        assert_eq!(short_auth_string("deadbeef"), "N3YN");
    }
}
