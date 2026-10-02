//! `vault.db` SQLite schema (spec v0.4 §3.2; v0.5 §22.7 `user_version =
//! 3`) and the corruption-mapping rules of §3.6.

use std::path::Path;

use rusqlite::Connection;

use crate::errors::ErrorCode;

pub const DB_NAME: &str = "vault.db";

/// The schema version this build reads and writes (v0.5: 3; a v2 vault is
/// migrated in place, a v1 one refused).
pub const USER_VERSION: u32 = 3;

/// v0.5 §22.7 / wire annex A.4: where each revision came from, and the
/// peer replay cache.
const V3: &str = "
CREATE TABLE rev_sources (              -- 'own', 'provider' or a 16-byte device id
  revision_id BLOB NOT NULL,
  source      BLOB NOT NULL,
  PRIMARY KEY (revision_id, source)
);
CREATE TABLE peer_inbox (               -- A.3.5: a LOCKED Mac's received puts
  id          INTEGER PRIMARY KEY,
  sender      BLOB NOT NULL,
  body        BLOB NOT NULL,
  objects     INTEGER NOT NULL,
  received_at INTEGER NOT NULL
);
CREATE TABLE refused_peer (             -- §22.7 cutoff: refused for good
  revision_id BLOB PRIMARY KEY
);
CREATE TABLE peer_replay (
  sender      BLOB NOT NULL,
  n           BLOB NOT NULL,
  received_at INTEGER NOT NULL,
  PRIMARY KEY (sender, n)
);
";

/// §3.2 v0.4: admitted revisions keyed by stable `revision_id`, the heads
/// (tip / conflict set), pending revisions, per-record freeze flags,
/// refusal counts, this device's author high-water mark, the import log
/// and helper-internal kv.
const SCHEMA: &str = "
CREATE TABLE record_revs (
  revision_id   BLOB PRIMARY KEY CHECK(length(revision_id)=32),
  record_id     TEXT NOT NULL,
  parent_ids    BLOB NOT NULL,
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
  record_id   TEXT NOT NULL,
  revision_id BLOB NOT NULL,
  PRIMARY KEY (record_id, revision_id)
);
CREATE TABLE pending_revs (
  revision_id BLOB PRIMARY KEY,
  record_id   TEXT NOT NULL,
  object      BLOB NOT NULL
);
CREATE TABLE record_flags (
  record_id TEXT PRIMARY KEY,
  frozen    INTEGER NOT NULL,
  evidence  BLOB NOT NULL
);
CREATE TABLE refused_revs (
  record_id TEXT NOT NULL,
  reason    INTEGER NOT NULL,
  count     INTEGER NOT NULL,
  PRIMARY KEY (record_id, reason)
);
CREATE TABLE author_hwm (
  record_id TEXT PRIMARY KEY,
  counter   INTEGER NOT NULL
);
CREATE TABLE import_log (
  fingerprint BLOB PRIMARY KEY,
  identity_ct BLOB NOT NULL
);
CREATE TABLE kv (
  key   TEXT PRIMARY KEY,
  value BLOB NOT NULL
);
CREATE TABLE refused_once (             -- a refused revision is counted once
  revision_id BLOB PRIMARY KEY
);
CREATE TABLE revoked_authors (          -- §3.2: D revoked; Admit(D) fixed
  author TEXT PRIMARY KEY
);
CREATE TABLE admitted_by_revoked (      -- Admit(D) revision ids
  author      TEXT NOT NULL,
  revision_id BLOB NOT NULL,
  PRIMARY KEY (author, revision_id)
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
        conn.execute_batch(V3).map_err(|_| ErrorCode::DbCorrupt)?;
        conn.pragma_update(None, "user_version", USER_VERSION)
            .map_err(|_| ErrorCode::DbCorrupt)?;
        return Ok(conn);
    }
    // §3.5: unknown newer schema → fail closed.
    let user_version: u32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|_| ErrorCode::DbCorrupt)?;
    if user_version == 2 {
        migrate_v2(&conn)?;
        return Ok(conn);
    }
    if user_version != USER_VERSION {
        // v0.4 is a clean break: a v1 database is refused, not migrated.
        return Err(if user_version > USER_VERSION {
            ErrorCode::FormatTooNew
        } else {
            ErrorCode::FormatInvalid
        });
    }
    Ok(conn)
}

/// v2 → v3 in one transaction: the new tables, and every existing
/// revision marked `own` (this device authored it) or `provider` — before
/// v0.5 no other path existed (§22.7).
fn migrate_v2(conn: &Connection) -> Result<(), ErrorCode> {
    let sql = format!(
        "BEGIN IMMEDIATE;{V3}
         INSERT INTO rev_sources (revision_id, source)
           SELECT revision_id,
                  CASE WHEN CAST(author_device AS BLOB) = (SELECT value FROM kv WHERE key = 'author_device')
                       THEN CAST('own' AS BLOB) ELSE CAST('provider' AS BLOB) END
           FROM record_revs;
         PRAGMA user_version = {USER_VERSION};
         COMMIT;"
    );
    conn.execute_batch(&sql).map_err(|_| {
        let _ = conn.execute_batch("ROLLBACK;");
        ErrorCode::DbCorrupt
    })
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
        assert_eq!(user_version, USER_VERSION);
        // All §3.2 tables exist.
        for table in [
            "record_revs",
            "record_tips",
            "record_conflicts",
            "pending_revs",
            "record_flags",
            "refused_revs",
            "author_hwm",
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
            conn.pragma_update(None, "user_version", 4u32).unwrap();
        }
        assert_eq!(
            open_db(&path, false).map(|_| ()),
            Err(ErrorCode::FormatTooNew)
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    /// v0.5: a v2 database is migrated in place — the provenance and
    /// replay tables appear and every existing revision gets a source.
    #[test]
    fn a_v2_database_migrates_to_v3() {
        let path = tmp_db("v2");
        {
            let conn = open_db(&path, true).unwrap();
            conn.execute_batch(
                "DROP TABLE rev_sources; DROP TABLE peer_replay; DROP TABLE peer_inbox; DROP TABLE refused_peer;",
            )
            .unwrap();
            // As production stores it (`set_author_device`): a BLOB.
            conn.execute("INSERT INTO kv (key, value) VALUES ('author_device', ?1)", [b"me".to_vec()]).unwrap();
            conn.execute(
                "INSERT INTO record_revs VALUES (?1, 'r', X'', 'me', 1, 0, 1, 1, 1, ?2, X'00', ?2, X'00', 0, 0)",
                rusqlite::params![vec![7u8; 32], vec![0u8; 24]],
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 2u32).unwrap();
        }
        let conn = open_db(&path, false).unwrap();
        let v: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, USER_VERSION);
        let src: Vec<u8> = conn.query_row("SELECT source FROM rev_sources", [], |r| r.get(0)).unwrap();
        assert_eq!(src, b"own");
        let n: i64 = conn.query_row("SELECT count(*) FROM peer_replay", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
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
