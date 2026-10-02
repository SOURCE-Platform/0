//! The vault engine (spec v0.5 §22.2, owner decision F2-D1: one Rust
//! engine on the Mac and the iPhone). Everything platform-neutral lives
//! here — storage, sync, the peer protocol, crypto, the registry, recovery
//! and the op logic, written against the `vault::Deps` traits for the
//! secure panel, presence and events. The macOS shell (`vault-helper`:
//! IPC, AppKit panel, LocalAuthentication, notifications) and the SOURCE
//! Vault app supply those services. The §2.12 Apple bridge (CryptoKit and
//! the Secure Enclave, `build.rs`) exists on both platforms.

pub mod backup;
pub mod crypto;
pub mod device;
pub mod enroll;
pub mod errors;
pub mod keychain;
pub mod peer;
pub mod recovery;
pub mod registry;
pub mod state;
pub mod storage;
pub mod sync;
#[cfg(debug_assertions)]
pub mod test_support;
pub mod vault;

/// Every committed cross-language vector family (§16.8): the Phase B
/// crypto families plus Phase E's enrollment/envelope contracts.
pub fn all_vectors() -> Vec<(&'static str, serde_json::Value)> {
    let mut all = crypto::vectors::all();
    all.push(("xv_enroll", enroll::vectors::xv_enroll()));
    all.extend(crypto::vectors_v04::all());
    all
}

/// Presence of this file in the vault directory means a vault exists.
pub const VAULT_HEADER_NAME: &str = "header.json";

/// Append-only device registry log (spec §4).
pub const VAULT_REGISTRY_NAME: &str = "registry.json";
