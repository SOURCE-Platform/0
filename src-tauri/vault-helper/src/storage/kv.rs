//! Small persistent records in the `kv` table (JSON values): sync
//! bookkeeping, remote-completion status (spec v0.4 §11.3.2). Writes take
//! the caller's connection so they join its transaction.

use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::errors::ErrorCode;

pub fn get<T: DeserializeOwned>(conn: &Connection, key: &str) -> Result<Option<T>, ErrorCode> {
    let v: Option<Vec<u8>> = conn
        .query_row("SELECT value FROM kv WHERE key=?1", params![key], |r| r.get(0))
        .optional()
        .map_err(|_| ErrorCode::DbCorrupt)?;
    v.map(|b| serde_json::from_slice(&b).map_err(|_| ErrorCode::DbCorrupt)).transpose()
}

pub fn put<T: Serialize>(conn: &Connection, key: &str, value: &T) -> Result<(), ErrorCode> {
    let b = serde_json::to_vec(value).map_err(|_| ErrorCode::Internal)?;
    conn.execute(
        "INSERT INTO kv (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, b],
    )
    .map(|_| ())
    .map_err(|_| ErrorCode::DbCorrupt)
}

pub fn delete(conn: &Connection, key: &str) -> Result<(), ErrorCode> {
    conn.execute("DELETE FROM kv WHERE key=?1", params![key]).map(|_| ()).map_err(|_| ErrorCode::DbCorrupt)
}
