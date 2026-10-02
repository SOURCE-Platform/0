//! Mac ↔ iPhone enrollment (§5). The helper owns every secret and every
//! decision; the main process owns the socket and relays frames it cannot
//! read into anything useful.
//!
//! One session at a time: `begin_enrollment` mints a single-use secret
//! and a nonce, `hello` authenticates the phone's first frame and fixes
//! the transcript, the user compares the SAS on both screens, `confirm`
//! (behind an LA presence check) builds the signed enroll entry and the
//! HPKE envelope, and `ack` — the phone's signature over the registry
//! head — is what finally writes the entry. A session that fails, expires
//! or is cancelled leaves no registry trace (§5.2 "Cancellation").

pub mod session;
pub mod transcript;
pub mod vectors;
pub mod wire;

pub use session::{EnrollSession, Stage, MAX_FAILURES, SESSION_TTL};
pub use transcript::{ack_digest, encode_secret, sas, transcript, Binding, SAS_LEN};
