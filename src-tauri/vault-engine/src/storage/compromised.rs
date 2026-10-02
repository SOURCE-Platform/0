//! Persistent COMPROMISED evidence (spec v0.4 §13.3: "COMPROMISED, which
//! is re-entered at open"; §4.6). Fork evidence found by a sync outlives
//! lock and restart: every later open of the vault enters COMPROMISED —
//! reads allowed, writes frozen — until the (v0.4-unspecified) exit
//! procedure clears it. A provider that later serves only one branch
//! cannot make the device forget what it saw.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::kv;
use crate::errors::ErrorCode;

const KEY: &str = "compromised";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// The §15 code that caused it (e.g. `REGISTRY_FORK`).
    pub code: String,
    pub at: u64,
}

pub fn mark(conn: &Connection, code: ErrorCode) -> Result<(), ErrorCode> {
    if load(conn)?.is_some() {
        return Ok(()); // the first evidence is kept
    }
    kv::put(conn, KEY, &Evidence { code: code.as_str().to_string(), at: super::store::now_epoch() })
}

pub fn load(conn: &Connection) -> Result<Option<Evidence>, ErrorCode> {
    kv::get(conn, KEY)
}
