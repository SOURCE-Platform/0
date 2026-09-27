//! Main-process backup coordinator (spec v0.4 §11.1, §11.3.2, §11.5):
//! drives the helper's provider ops and moves ciphertext between the
//! helper and the provider. It never sees plaintext, MP/PK/RK, the VK or
//! any private key, and holds no reusable authentication secret — every
//! request is signed by the helper, one at a time.
//!
//! Two traits keep it testable: `Helper` (one op frame in, one response
//! out — the app's vault connection, or the in-process dispatcher in
//! tests) and `Transport` (one HTTPS request — URLSession in the app, the
//! in-process provider core in tests).

pub mod flows;
pub mod policy;

use serde_json::Value;

/// One helper op: the full response frame (`ok:false` included).
pub trait Helper: Send + Sync {
    fn op(&self, frame: Value) -> Result<Value, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
    /// The HTTP `Date` header as Unix seconds (clock-skew check, §11.3.2).
    pub date: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// No network or the provider could not be reached.
    Unreachable(String),
}

/// One HTTPS request to the provider. Standard platform certificate
/// validation, no pinning (§11.1).
pub trait Transport: Send + Sync {
    fn send(&self, origin: &str, method: &str, path: &str, auth: Option<&str>, body: &[u8]) -> Result<HttpResponse, TransportError>;
}

/// Why a coordinator run stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The helper refused an op (its §15 code).
    Helper(String),
    /// The provider answered with an error (status, §11.3 code).
    Provider(u16, String),
    /// The provider could not be reached.
    Unreachable,
    /// §11.3: three merges in a row found the state moved again.
    Conflict,
}

pub type Outcome<T> = Result<T, Failure>;

pub(crate) fn code_of(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body).ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_default()
}

pub(crate) fn ok(r: Result<Value, String>) -> Outcome<Value> {
    match r {
        Ok(v) if v["ok"] == true => Ok(v),
        Ok(v) => Err(Failure::Helper(v["error"].as_str().unwrap_or("INTERNAL").to_string())),
        Err(e) => Err(Failure::Helper(e)),
    }
}
