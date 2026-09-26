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
