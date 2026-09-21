//! Mac side of §5 device enrollment: the ephemeral TLS server the phone
//! connects to, and the session state the Vault tab drives.
//!
//! This process owns the socket and nothing else. Every frame it receives
//! goes to the helper unmodified; the helper verifies the secret, fixes
//! the transcript, derives the SAS, signs the registry entry and seals
//! the envelope. The server exists only for the duration of one
//! enrollment: a fresh certificate, a port the OS picks, and a shutdown
//! as soon as the flow ends, succeeds, fails or times out (§5: "the
//! always-on mobile server is not enrollment attack surface").

pub mod server;
pub mod session;

pub use session::{
    begin, cancel, confirm, sas, status, EnrollmentPayloadV2, Session, SESSION_TTL,
};
