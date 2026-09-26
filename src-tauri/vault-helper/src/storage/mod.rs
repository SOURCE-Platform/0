//! Vault on-disk storage (spec §3). Only the helper opens anything under
//! the vault directory (§3.1).
//!
//! - `header` — `header.json` model (§2.10 fields, §2.3 kdf block)
//! - `db` — `vault.db` SQLite schema and corruption mapping (§3.2/§3.6)
//! - `manifest` — local `manifest.json` + the §3.5 mismatch refusal
//! - `records` — login/card plaintext models and validation (§8)
//! - `revisions` — content-committed revision graph (§3.2)
//! - `store` — the `VaultStore` tying them together at open/unlock time

pub mod adopt;
pub mod db;
pub mod flip;
pub mod header;
pub mod import_log;
pub mod kv;
pub mod manifest;
pub mod merge;
pub mod records;
pub mod rev_state;
pub mod revision_rows;
pub mod revisions;
pub mod revoked;
pub mod rotation;
pub mod rotation_journal;
pub mod store;
pub mod store_list;
pub mod store_records;

pub use store::VaultStore;
