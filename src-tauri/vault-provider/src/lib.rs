//! Library half of `vault-provider` (the binary is a thin launcher), so
//! the HTTP adapter can be exercised end to end in tests.

pub mod config;
pub mod http;
pub mod s3;

use std::collections::BTreeSet;
use std::sync::Arc;

use config::{Config, Store};
use vault_provider_core::fs::FsStores;
use vault_provider_core::{Config as CoreConfig, Provider};

/// The provider over the configured stores, plus the ops store (for the
/// GC pass's vault listing).
pub fn build(cfg: Config) -> (Arc<Provider>, Arc<dyn vault_provider_core::OpsStore>) {
    let core = CoreConfig::new(&cfg.origin, cfg.pepper);
    match cfg.store {
        Store::Fs(dir) => {
            let fs = Arc::new(FsStores::new(std::path::Path::new(&dir)));
            (Arc::new(Provider::with_fs(core, fs.clone())), fs)
        }
        Store::S3(s3cfg) => {
            let c = Arc::new(s3::S3Client::new(s3cfg));
            (Arc::new(Provider::new(core, c.clone(), c.clone(), c.clone())), c)
        }
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Every vault id with a state object (FsStores lists one directory
/// level; S3 lists every key under the prefix).
fn vault_ids(ops: &dyn vault_provider_core::OpsStore) -> BTreeSet<[u8; 16]> {
    ops.list_prefix("v2/vaults/")
        .unwrap_or_default()
        .iter()
        .filter_map(|k| k.strip_prefix("v2/vaults/")?.split('/').next().and_then(vault_proto::crypto::hex::decode_array::<16>))
        .collect()
}

pub fn gc(cfg: Config) -> Result<(), String> {
    let (p, ops) = build(cfg);
    for vid in vault_ids(ops.as_ref()) {
        match p.gc_vault(&vid, now()) {
            Ok(n) => eprintln!("gc {} deleted {n}", vault_proto::crypto::hex::encode(vid)),
            Err(e) => eprintln!("gc {} failed {}", vault_proto::crypto::hex::encode(vid), e.as_str()),
        }
    }
    Ok(())
}


/// The service's router over a provider.
pub fn router(p: Arc<Provider>) -> axum::Router {
    axum::Router::new().fallback(http::handle).with_state(p)
}
