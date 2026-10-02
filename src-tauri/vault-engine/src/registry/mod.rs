//! Device registry (spec §4): verification of the append-only chain,
//! construction of entries, and the device-identity abstraction the
//! registry and signed manifests build on. The entry codec, hashes, and
//! proof primitives live in `crypto::registry` (Phase B).

pub mod log;

// Moved to `vault-proto` (spec v0.4 §1.1); re-exported at their old paths.
pub use vault_proto::registry::{build, chain, device, file};
