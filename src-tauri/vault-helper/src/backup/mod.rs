//! Backup format and the Phase D rehearsal store (spec §3.7, §11). The
//! production provider (`HttpBackupStore`) is Phase F.

pub mod finalize;
pub mod fs_recovery;
pub mod fs_store;
/// The Phase D JSON index, kept until the helper moves to index v2 (Phase F).
pub mod index_v1;
pub mod snapshot;

// Moved to `vault-proto` (spec v0.4 §1.1); re-exported at their old paths.
pub use vault_proto::backup::{checkpoint, manifest, object};
