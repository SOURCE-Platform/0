use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub version: String,
    pub device_name: String,
    pub tls_fingerprint_sha256: String,
}

/// Phone asks to pair. `observed_fingerprint` is the SHA-256 of the certificate
/// the phone actually received — a cheap early reject when it disagrees with ours.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairStartRequest {
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub device_name: String,
    #[serde(default)]
    pub observed_fingerprint: String,
    /// Single-use secret read from the QR code on the Mac's screen. When it
    /// checks out, the phone already proved physical presence and no dialog is
    /// raised. Absent for the fallback Allow flow.
    #[serde(default)]
    pub secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairStartResponse {
    pub request_id: String,
    /// Set when a valid QR secret short-circuited the approval dialog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
}

/// Deliberately carries no comparison tag: the phone must derive its own from
/// the certificate it sees, or an impersonator could simply echo the real one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairPollResponse {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipUploadMeta {
    pub clip_id: String,
    pub device_id: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamOpen {
    Open {
        clip_id: String,
        device_id: String,
        started_at_ms: i64,
        sample_rate: u32,
    },
    Close {
        clip_id: String,
        ended_at_ms: i64,
    },
}

#[derive(Debug, Clone)]
pub struct AuthDevice {
    pub device_id: String,
    pub device_name: String,
}
