//! Every provider semantic of spec v0.4 §11 over three storage traits
//! (`StateStore`, `BlobStore`, `OpsStore`), plus `FsStores` for tests and
//! rehearsals. The provider is untrusted for confidentiality and is not a
//! root of trust: it stores ciphertext and public data and enforces
//! structure as defense in depth; clients verify everything they accept.

use std::sync::Arc;

pub mod auth;
pub mod claim;
pub mod commit;
pub mod fs;
pub mod gc;
pub mod locate;
pub mod model;
pub mod push;
pub mod registry_rules;
pub mod service;
pub mod stores;
pub mod throttle;
pub mod validate;

pub use model::Response;
pub use service::Request;
pub use stores::{BlobStore, OpsStore, StateStore};

pub struct Config {
    /// This provider's origin (`ProviderRequest.audience` must equal it).
    pub origin: String,
    /// Locate pepper (§11.5): enumeration mitigation only, no vault authority.
    pub pepper: [u8; 32],
    /// MP-class throttle: L slots per window of W seconds (§11.5).
    pub throttle_slots: u32,
    pub throttle_window: u64,
}

impl Config {
    pub fn new(origin: &str, pepper: [u8; 32]) -> Config {
        Config { origin: origin.to_string(), pepper, throttle_slots: 10, throttle_window: 3600 }
    }
}

pub struct Provider {
    pub cfg: Config,
    pub(crate) state: Arc<dyn StateStore>,
    pub(crate) blobs: Arc<dyn BlobStore>,
    pub(crate) ops: Arc<dyn OpsStore>,
    read_nonces: auth::ReadNonces,
    locate_limits: locate::LocateLimits,
    /// Phase G (§7.2): APNs, when configured (`with_push`).
    pub(crate) push: Option<Arc<dyn push::PushSender>>,
    pub(crate) push_limits: push::PushLimits,
}

impl Provider {
    pub fn new(cfg: Config, state: Arc<dyn StateStore>, blobs: Arc<dyn BlobStore>, ops: Arc<dyn OpsStore>) -> Provider {
        Provider { cfg, state, blobs, ops, read_nonces: Default::default(), locate_limits: Default::default(), push: None, push_limits: Default::default() }
    }

    /// The push relay's sender (APNs in production; a recording sender in
    /// tests). Without one, `push/send` answers `BACKUP_UNAVAILABLE`.
    pub fn with_push(mut self, sender: Arc<dyn push::PushSender>) -> Provider {
        self.push = Some(sender);
        self
    }

    /// A provider over one `FsStores` tree.
    pub fn with_fs(cfg: Config, fs: Arc<fs::FsStores>) -> Provider {
        Provider::new(cfg, fs.clone(), fs.clone(), fs)
    }
}
