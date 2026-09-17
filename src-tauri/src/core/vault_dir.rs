//! Filesystem home for the future credential vault.
//!
//! The directory is created empty and protected ahead of time so indexing
//! and backup layers learn the boundary before any vault state exists. This
//! module intentionally creates no vault content — no keys, no database, no
//! config. See `docs/security/credential-vault-security-architecture.md` §7.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const VAULT_DIR_NAME: &str = "vault";

/// Presence of this file tells Spotlight's metadata importer to skip the
/// directory entirely.
pub const SPOTLIGHT_NEVER_INDEX: &str = ".metadata_never_index";

/// The future vault directory inside the SOURCE data directory.
pub fn vault_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(VAULT_DIR_NAME)
}

/// Create the future vault directory with owner-only permissions and a
/// Spotlight never-index marker. Idempotent; safe to call on every launch.
pub fn ensure_future_vault_dir(data_dir: &Path) -> io::Result<PathBuf> {
    let dir = vault_dir(data_dir);
    fs::create_dir_all(&dir)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }

    let marker = dir.join(SPOTLIGHT_NEVER_INDEX);
    if !marker.exists() {
        fs::write(&marker, b"")?;
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_data_dir() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("source-vault-dir-test")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&dir).expect("create temp data dir");
        dir
    }

    #[test]
    fn creates_directory_with_spotlight_marker() {
        let data_dir = fresh_data_dir();
        let dir = ensure_future_vault_dir(&data_dir).expect("ensure vault dir");
        assert!(dir.is_dir());
        assert_eq!(dir, vault_dir(&data_dir));
        assert!(dir.join(SPOTLIGHT_NEVER_INDEX).is_file());
        let _ = fs::remove_dir_all(&data_dir);
    }

    #[test]
    #[cfg(unix)]
    fn directory_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let data_dir = fresh_data_dir();
        let dir = ensure_future_vault_dir(&data_dir).expect("ensure vault dir");
        let mode = dir.metadata().expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "vault dir must be owner-only");
        let _ = fs::remove_dir_all(&data_dir);
    }

    #[test]
    fn is_idempotent_and_preserves_contents() {
        let data_dir = fresh_data_dir();
        let dir = ensure_future_vault_dir(&data_dir).expect("first ensure");
        let sentinel = dir.join("sentinel");
        fs::write(&sentinel, b"keep").expect("write sentinel");
        ensure_future_vault_dir(&data_dir).expect("second ensure");
        assert_eq!(fs::read(&sentinel).expect("read sentinel"), b"keep");
        let _ = fs::remove_dir_all(&data_dir);
    }
}
