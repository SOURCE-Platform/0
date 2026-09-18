//! `vault.db` SQLite schema (spec §3.2, `user_version = 1`) and the
//! corruption-mapping rules of §3.6.

use std::path::Path;

use rusqlite::Connection;

use crate::errors::ErrorCode;

pub const DB_NAME: &str = "vault.db";

/// §3.2 verbatim: every revision of every record, current tips, open
/// conflicts, import idempotency log, and helper-internal kv.
const SCHEMA: &str = "
CREATE TABLE record_revs (
  rev_hash      BLOB PRIMARY KEY,
  record_id     TEXT NOT NULL,
  parent_revs   BLOB NOT NULL,
  author_device TEXT NOT NULL,
  counter       INTEGER NOT NULL,
  deleted       INTEGER NOT NULL DEFAULT 0,
  kind_tag      INTEGER NOT NULL,
  vk_generation INTEGER NOT NULL,
  schema_version INTEGER NOT NULL,
  nonce         BLOB NOT NULL CHECK(length(nonce)=24),
  ct            BLOB NOT NULL,
  meta_nonce    BLOB NOT NULL CHECK(length(meta_nonce)=24),
  meta_ct       BLOB NOT NULL,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);
CREATE INDEX idx_revs_record ON record_revs(record_id);
CREATE TABLE record_tips (
  record_id TEXT PRIMARY KEY,
  tip_rev   BLOB
);
CREATE TABLE record_conflicts (
  record_id TEXT NOT NULL,
  rev_hash  BLOB NOT NULL,
  PRIMARY KEY (record_id, rev_hash)
);
CREATE TABLE import_log (
  fingerprint BLOB PRIMARY KEY,
  identity_ct BLOB NOT NULL
);
CREATE TABLE kv (
  key   TEXT PRIMARY KEY,
  value BLOB NOT NULL
);
";

/// Open (or create) the database and enforce the §3.2 pragmas.
/// `create=true` runs the schema DDL for a new vault.
pub fn open_db(path: &Path, create: bool) -> Result<Connection, ErrorCode> {
    let conn = Connection::open(path).map_err(|_| ErrorCode::DbCorrupt)?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|_| ErrorCode::DbCorrupt)?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|_| ErrorCode::DbCorrupt)?;
    // §3.2: deleted rows are overwritten, not merely unlinked.
    conn.pragma_update(None, "secure_delete", "ON")
        .map_err(|_| ErrorCode::DbCorrupt)?;
    if create {
        conn.execute_batch(SCHEMA)
            .map_err(|_| ErrorCode::DbCorrupt)?;
        conn.pragma_update(None, "user_version", 1u32)
            .map_err(|_| ErrorCode::DbCorrupt)?;
        return Ok(conn);
    }
    // §3.5: unknown newer schema → fail closed.
    let user_version: u32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|_| ErrorCode::DbCorrupt)?;
    if user_version != 1 {
        return Err(if user_version > 1 {
            ErrorCode::FormatTooNew
        } else {
            ErrorCode::DbCorrupt
        });
    }
    Ok(conn)
}

/// §3.6 open sequence step: full-page integrity check. Failure maps to
/// `DB_CORRUPT` → ERROR state at the op layer; the helper never deletes
/// the file.
pub fn integrity_check(conn: &Connection) -> Result<(), ErrorCode> {
    let result: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|_| ErrorCode::DbCorrupt)?;
    if result == "ok" {
        Ok(())
    } else {
        Err(ErrorCode::DbCorrupt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_db(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vh-db-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(DB_NAME)
    }

    #[test]
    fn create_then_open_with_schema_version() {
        let path = tmp_db("create");
        {
            let conn = open_db(&path, true).unwrap();
            integrity_check(&conn).unwrap();
        }
        let conn = open_db(&path, false).unwrap();
        let user_version: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(user_version, 1);
        // All §3.2 tables exist.
        for table in [
            "record_revs",
            "record_tips",
            "record_conflicts",
            "import_log",
            "kv",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "missing table {table}");
        }
        let journal: String = conn
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(journal, "wal");
        let secure: i64 = conn
            .pragma_query_value(None, "secure_delete", |r| r.get(0))
            .unwrap();
        assert_eq!(secure, 1);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn newer_user_version_refused_format_too_new() {
        let path = tmp_db("toonew");
        {
            let conn = open_db(&path, true).unwrap();
            conn.pragma_update(None, "user_version", 2u32).unwrap();
        }
        assert_eq!(
            open_db(&path, false).map(|_| ()),
            Err(ErrorCode::FormatTooNew)
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn garbage_file_is_db_corrupt() {
        let path = tmp_db("garbage");
        std::fs::write(&path, b"this is not sqlite").unwrap();
        // SQLite may not notice until a query touches the schema.
        let result = open_db(&path, false).and_then(|c| integrity_check(&c));
        assert_eq!(result, Err(ErrorCode::DbCorrupt));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
