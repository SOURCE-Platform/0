//! Serving peer requests (spec v0.5 §22.8; wire annex revision 3): the
//! Mac answers; the iPhone asks. `verify` runs the §22.8 receiver order
//! and returns either an accepted request or an unsigned refusal;
//! `respond` builds and signs every response itself; `ops` answers each
//! operation. Nothing here signs a caller-supplied digest.

pub mod admit;
pub mod graph;
pub mod inbox;
pub mod ops;
pub mod respond;
pub mod serve_revs;
pub mod verify;

use std::path::PathBuf;

use crate::device::SeDevice;

/// What the serving helper is, for this one request.
pub struct Ctx {
    pub dir: PathBuf,
    pub vault_id: [u8; 16],
    pub me: SeDevice,
    /// §22.14: a behind Mac answers only hello and status.
    pub behind: bool,
    /// COMPROMISED: no `peer_revs_put`.
    pub compromised: bool,
    /// LOCKED: provider-confirmed revisions only; pushes go to the inbox.
    pub locked: bool,
    /// §22.7 freshness: a verified provider `state_get` in the last 15 min.
    pub fresh: bool,
}

/// Unsigned refusals (wire annex A.2.1): the HTTP status main returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// 403 — any authentication failure, a replay, who-may-speak, a body
    /// that does not hash to `body_sha256`.
    Forbidden,
    /// 429 — the per-sender request rate.
    Rate,
    /// 503 — not serving, or cannot sign right now.
    Unavailable,
}

impl Refusal {
    pub fn http(self) -> u16 {
        match self {
            Refusal::Forbidden => 403,
            Refusal::Rate => 429,
            Refusal::Unavailable => 503,
        }
    }
}
