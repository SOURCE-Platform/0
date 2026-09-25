//! SOURCE vault helper — Phase A skeleton (implementation spec §18 Phase A).
//!
//! This crate implements the *boundary* the rest of the vault is built on:
//! a separately signed `SourceVaultHelper.app` serving length-prefixed JSON
//! frames over a Unix-domain socket, with bidirectional SecCode peer
//! authentication (spec §1.4) and the `hello` / `get_state` / `lock` ops
//! (spec §1.5).
//!
//! Phase A deliberately contains **no** vault keys, no credential storage,
//! no cryptography, and no ops beyond the three above. Anything else is
//! answered `UNKNOWN_OP` (see `ops`).

pub mod backup;
pub mod crypto;
pub mod device;
pub mod enroll;
pub mod errors;
pub mod ffi;
pub mod ipc;
pub mod keychain;
pub mod la;
#[macro_use]
pub mod log;
pub mod notify;
pub mod ops;
pub mod panel;
pub mod recovery;
pub mod registry;
pub mod state;
pub mod storage;
#[cfg(debug_assertions)]
pub mod test_support;
pub mod vault;

/// Every committed cross-language vector family (§16.8): the Phase B
/// crypto families plus Phase E's enrollment/envelope contracts.
pub fn all_vectors() -> Vec<(&'static str, serde_json::Value)> {
    let mut all = crypto::vectors::all();
    all.push(("xv_enroll", enroll::vectors::xv_enroll()));
    all
}

/// IPC protocol major version (spec §1.4). Peers with a different major
/// version are disconnected at `hello`.
pub const PROTO_VERSION: u32 = 1;

/// Default vault data directory; must match the main app's `vault_dir()`.
pub const DEFAULT_VAULT_DIR: &str = ".observer_data/vault";

/// Socket file name inside the vault directory (spec §1.4).
pub const SOCKET_NAME: &str = "helper.sock";

/// Presence of this file in the vault directory means a vault exists.
pub const VAULT_HEADER_NAME: &str = "header.json";

/// Append-only device registry log (spec §4). Phase C creates it empty;
/// the genesis entry lands with Phase E enrollment.
pub const VAULT_REGISTRY_NAME: &str = "registry.json";

/// Helper exits after this long with zero connected clients (spec §1.6).
pub const IDLE_EXIT_SECS: u64 = 30 * 60;

/// Grace period for clients to disconnect during shutdown (spec §1.6).
pub const SHUTDOWN_GRACE_SECS: u64 = 5;

/// Resolve the vault directory. The `OV0_VAULT_DIR` override exists for
/// development and gate testing and is compiled to the default in release.
pub fn vault_dir() -> std::path::PathBuf {
    #[cfg(debug_assertions)]
    if let Ok(dir) = std::env::var("OV0_VAULT_DIR") {
        if !dir.is_empty() {
            return std::path::PathBuf::from(dir);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::Path::new(&home).join(DEFAULT_VAULT_DIR)
}

/// Resolve the socket path. Same debug-only override policy as `vault_dir`.
pub fn socket_path() -> std::path::PathBuf {
    #[cfg(debug_assertions)]
    if let Ok(path) = std::env::var("OV0_VAULT_SOCKET_PATH") {
        if !path.is_empty() {
            return std::path::PathBuf::from(path);
        }
    }
    vault_dir().join(SOCKET_NAME)
}
