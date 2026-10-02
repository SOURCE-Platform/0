//! Backup formats (spec v0.4 §3.7, §11.2). The formats live in
//! `vault-proto`; the helper's publish/sync/recovery engine is `sync`.

// Moved to `vault-proto` (spec v0.4 §1.1); re-exported at their old paths.
pub use vault_proto::backup::{checkpoint, index, manifest, object, stage};
