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
    /// Bumped by every `add`: a staging records the version it carried,
    /// and its commit clears only that version (a newer change stays).
    #[serde(default)]
    pub version: u64,
    /// Stagings posted but not yet known to have landed (a lost `200`,
    /// a restart): recognized when a verified served state carries their
    /// singletons (§11.3.2 "state_get shows … the change").
    #[serde(default)]
    pub in_flight: Vec<InFlight>,
}

/// One staged transition carrying this record, as posted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InFlight {
    pub version: u64,
    /// The singletons the provider holds once it commits.
    pub base_after: Base,
}

const IN_FLIGHT_KEEP: usize = 4;

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
    // A change made over a record that needs the user is the redo: it
    // starts afresh on the adopted base (§11.3 singleton rule).
    let prior = load(conn)?;
    let version = prior.as_ref().map_or(0, |p| p.version) + 1;
    let mut p = prior.filter(|p| !p.needs_user).unwrap_or(PendingRemote {
        ops: Vec::new(),
        security_driven: false,
        local_committed_at: now,
        recovery_auth_updates: Vec::new(),
        base,
        needs_user: false,
        attempts: 0,
        last_error: None,
        version: 0,
        in_flight: Vec::new(),
    });
    p.version = version;
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

/// The base a new change builds on: the pending record's, unless it needs
/// the user (then the adopted, current header is the base).
pub fn base_for(conn: &Connection, current: &Header) -> Result<Base, ErrorCode> {
    Ok(match load(conn)? {
        Some(p) if !p.needs_user => p.base,
        _ => Base::of(current),
    })
}

/// A staging carrying this record is about to be posted: remember it.
/// Returns the version it carries.
pub fn note_staged(conn: &Connection, base_after: Base) -> Result<Option<u64>, ErrorCode> {
    let Some(mut p) = load(conn)? else { return Ok(None) };
    let v = p.version;
    p.in_flight.retain(|f| f.version != v || f.base_after != base_after);
    p.in_flight.push(InFlight { version: v, base_after });
    let excess = p.in_flight.len().saturating_sub(IN_FLIGHT_KEEP);
    p.in_flight.drain(..excess);
    save(conn, &p)?;
    Ok(Some(v))
}

/// A transition carrying version `version` landed with singletons `landed`
/// (REMOTE_COMMITTED). The record clears only if nothing was added since;
/// otherwise it stays, rebased on what landed. Returns the cleared ops.
pub fn settle(conn: &Connection, version: u64, landed: Base) -> Result<Vec<PendingOp>, ErrorCode> {
    let Some(mut p) = load(conn)? else { return Ok(Vec::new()) };
    if p.version == version {
        clear(conn)?;
        return Ok(p.ops);
    }
    // Updates the landed state already carries are not re-sent (the
    // provider requires an update exactly when that class's salt moves).
    p.recovery_auth_updates.retain(|u| {
        let landed_salt = match crate::crypto::recovery_auth::RecoveryClass::from_code(u.class) {
            Some(crate::crypto::recovery_auth::RecoveryClass::Mp) => landed.auth_salt_mp,
            Some(crate::crypto::recovery_auth::RecoveryClass::Rk) => landed.auth_salt_rk,
            None => return true,
        };
        u.salt != landed_salt
    });
    p.base = landed;
    p.needs_user = false;
    p.attempts = 0;
    p.last_error = None;
    p.in_flight.retain(|f| f.version > version);
    save(conn, &p)?;
    Ok(Vec::new())
}

/// The in-flight staging whose singletons `landed` shows, if any.
pub fn landed(p: &PendingRemote, landed: &Base) -> Option<u64> {
    p.in_flight.iter().filter(|f| &f.base_after == landed).map(|f| f.version).max()
}

/// Whether the committed state still matches the pending change's base.
pub fn base_unchanged(p: &PendingRemote, committed: &Header) -> bool {
    Base::of(committed) == p.base
}
