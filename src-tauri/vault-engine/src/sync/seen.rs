//! The last provider state this device accepted (spec v0.4 §11.5, §11.7):
//! the rollback floor and fork reference for every later state, and the
//! `expected_state` of the next transition. Persisted in `kv`.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::crypto::recovery_auth::RecoveryClass;
use crate::errors::ErrorCode;
use crate::storage::header::{Hex16, Hex32};
use crate::storage::kv;
use vault_proto::state::RecoveryAuthEntry;

const KEY: &str = "remote_seen";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenAuth {
    pub class: u8,
    pub public: String,
    pub salt: Hex16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    pub generation: u64,
    pub manifest_hash: Hex32,
    pub state_commit: Hex32,
    /// The recovery-auth set of that state (public keys and salts).
    pub recovery_auth: Vec<SeenAuth>,
}

impl Seen {
    pub fn auth_entries(&self) -> Result<Vec<RecoveryAuthEntry>, ErrorCode> {
        self.recovery_auth
            .iter()
            .map(|a| {
                Ok(RecoveryAuthEntry {
                    class: RecoveryClass::from_code(a.class).ok_or(ErrorCode::DbCorrupt)?,
                    public: crate::crypto::hex::decode_array(&a.public).ok_or(ErrorCode::DbCorrupt)?,
                    salt: a.salt.0,
                })
            })
            .collect()
    }

    pub fn with_auth(generation: u64, manifest_hash: [u8; 32], state_commit: [u8; 32], auth: &[RecoveryAuthEntry]) -> Seen {
        let mut recovery_auth: Vec<SeenAuth> = auth
            .iter()
            .map(|e| SeenAuth { class: e.class.code(), public: crate::crypto::hex::encode(e.public), salt: Hex16(e.salt) })
            .collect();
        recovery_auth.sort_by_key(|a| a.class);
        Seen { generation, manifest_hash: Hex32(manifest_hash), state_commit: Hex32(state_commit), recovery_auth }
    }
}

/// Apply updates over a recovery-auth set (one entry per class).
pub fn merge_auth(base: &[RecoveryAuthEntry], updates: &[RecoveryAuthEntry]) -> Vec<RecoveryAuthEntry> {
    let mut out: Vec<RecoveryAuthEntry> = base.iter().filter(|b| !updates.iter().any(|u| u.class == b.class)).copied().collect();
    out.extend_from_slice(updates);
    out.sort_by_key(|e| e.class);
    out
}

pub fn load(conn: &Connection) -> Result<Option<Seen>, ErrorCode> {
    kv::get(conn, KEY)
}

pub fn save(conn: &Connection, seen: &Seen) -> Result<(), ErrorCode> {
    kv::put(conn, KEY, seen)
}

const BODY_KEY: &str = "seen_state_body";

/// Keep the verified `state_get` body of the accepted state, only if it is
/// exactly the state this device accepted (wire annex A.3.2).
pub fn keep_body(conn: &Connection, state_commit: &[u8; 32], body: &[u8]) -> Result<(), ErrorCode> {
    match load(conn)? {
        Some(s) if &s.state_commit.0 == state_commit && body.len() <= 64 * 1024 => {
            crate::storage::kv::put(conn, BODY_KEY, &crate::crypto::hex::encode(body))
        }
        _ => Ok(()),
    }
}

/// The kept body, if it still belongs to the accepted state (after our
/// own publication commits there is none until the next `state_get`).
pub fn body(conn: &Connection) -> Result<Option<Vec<u8>>, ErrorCode> {
    let Some(s) = load(conn)? else { return Ok(None) };
    let Some(hex_body): Option<String> = crate::storage::kv::get(conn, BODY_KEY)? else { return Ok(None) };
    let Some(bytes) = crate::crypto::hex::decode(&hex_body) else { return Ok(None) };
    let same = crate::sync::remote::parse(&bytes).is_ok_and(|r| r.state_commit == s.state_commit.0);
    Ok(same.then_some(bytes))
}
