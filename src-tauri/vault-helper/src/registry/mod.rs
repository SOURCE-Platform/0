//! Device registry (spec §4): verification of the append-only chain,
//! construction of entries, and the device-identity abstraction the
//! registry and signed manifests build on. The entry codec, hashes, and
//! proof primitives live in `crypto::registry` (Phase B).

pub mod build;
pub mod chain;
pub mod device;
pub mod file;
