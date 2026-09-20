//! Backup format and the Phase D rehearsal store (spec §3.7, §11). The
//! production provider (`HttpBackupStore`) is Phase F.

pub mod checkpoint;
pub mod finalize;
pub mod fs_recovery;
pub mod fs_store;
pub mod index;
pub mod manifest;
pub mod object;
pub mod snapshot;
