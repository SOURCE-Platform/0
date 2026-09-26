//! Remote-completion status (spec v0.4 §11.3.2): a wrap-bearing local
//! change (vault creation, MP change, RK replacement, revocation,
//! enrollment) is recorded in `kv` in the same local commit as the change,
//! with the `base` of the remote state it was built on. The status is
//! LOCAL_COMMITTED → REMOTE_UPDATE_PENDING until a transition containing
//! it commits (REMOTE_COMMITTED). It survives lock, restart and crash.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::seen::SeenAuth;
use crate::errors::ErrorCode;
use crate::storage::header::{Header, Hex16, Hex32};
use crate::storage::kv;

const KEY: &str = "pending_remote";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingOp {
    VaultCreate,
    MpChange,
    RkReplacement,
    Revocation,
    Enrollment,
}

/// The remote singletons a pending change was built on (§11.3 rule).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Base {
    pub vk_generation: u32,
    pub kdf_salt: Hex16,
    pub auth_salt_mp: Hex16,
    pub auth_salt_rk: Hex16,
    pub registry_head: Hex32,
}

impl Base {
    pub fn of(h: &Header) -> Base {
        Base {
            vk_generation: h.vk_generation,
            kdf_salt: h.kdf.salt,
            auth_salt_mp: h.auth_salt_mp,
            auth_salt_rk: h.auth_salt_rk,
            registry_head: h.registry_head,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRemote {
    /// Every component still pending (a partial resolution removes one).
    pub ops: Vec<PendingOp>,
    /// Sticky: a suspected-stolen RK or a revocation.
    pub security_driven: bool,
    pub local_committed_at: u64,
    /// Public; what a re-stage needs (§11.3.2).
    pub recovery_auth_updates: Vec<SeenAuth>,
    pub base: Base,
    /// The base changed remotely: the user must redo the operation.
    pub needs_user: bool,
    pub attempts: u32,
    pub last_error: Option<String>,
}

pub fn load(conn: &Connection) -> Result<Option<PendingRemote>, ErrorCode> {
    kv::get(conn, KEY)
}

pub fn save(conn: &Connection, p: &PendingRemote) -> Result<(), ErrorCode> {
    kv::put(conn, KEY, p)
}

pub fn clear(conn: &Connection) -> Result<(), ErrorCode> {
    kv::delete(conn, KEY)
}

/// Record one more pending component, merging with any already pending
/// (a second change while one is pending stages both; `security_driven`
/// is sticky, §11.3.2).
pub fn add(
    conn: &Connection,
    op: PendingOp,
    security_driven: bool,
    base: Base,
    updates: Vec<SeenAuth>,
    now: u64,
) -> Result<PendingRemote, ErrorCode> {
    let mut p = load(conn)?.unwrap_or(PendingRemote {
        ops: Vec::new(),
        security_driven: false,
        local_committed_at: now,
        recovery_auth_updates: Vec::new(),
        base,
        needs_user: false,
        attempts: 0,
        last_error: None,
    });
    if !p.ops.contains(&op) {
        p.ops.push(op);
    }
    p.security_driven |= security_driven;
    for u in updates {
        p.recovery_auth_updates.retain(|x| x.class != u.class);
        p.recovery_auth_updates.push(u);
    }
    p.recovery_auth_updates.sort_by_key(|u| u.class);
    save(conn, &p)?;
    Ok(p)
}

/// Whether the committed state still matches the pending change's base.
pub fn base_unchanged(p: &PendingRemote, committed: &Header) -> bool {
    Base::of(committed) == p.base
}
