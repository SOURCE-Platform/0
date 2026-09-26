//! Device registry (spec §4): structural chain verification, the registry
//! file codec, and the device-identity abstraction the registry and
//! signed manifests build on. The entry codec, hashes and proof primitives
//! live in `crypto::registry`.

pub mod build;
pub mod chain;
pub mod device;
pub mod file;
