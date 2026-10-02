//! A LOCKED Mac's bounded inbox (wire annex A.3.5): a `peer_revs_put`
//! that arrives while LOCKED is checked as far as it can be without the
//! key — decoding, canonical order, parent closure (review VER-I3) — and
//! stored as
//! received — ciphertext only, ≤ 2,000 revisions and 16 MiB per peer —
//! and admitted at the next unlock through the ordinary §22.7 path
//! (`admit::put`), after who-may-speak is checked again. A peer's inbox
//! is purged when its revocation is accepted.

use rusqlite::params;
use vault_proto::peer::exchange::Revs;

use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::storage::VaultStore;

pub const MAX_OBJECTS: i64 = 2_000;
pub const MAX_BYTES: i64 = 16 << 20;

fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    r.map_err(|_| ErrorCode::DbCorrupt)
}

/// Store a put for later. `Err(PeerLimit)` over the caps (status 2);
/// `Err(FormatInvalid)` for a malformed body (status 4). Returns the
/// number of revisions held.
pub fn stash(store: &VaultStore, sender: &[u8; 16], body: &[u8], now: u64) -> Result<u64, ErrorCode> {
    let batch = Revs::decode(body, true)?;
    let n = batch.objects.len() as i64;
    if n > MAX_OBJECTS {
        return Err(ErrorCode::PeerLimit);
    }
    let rows = super::admit::decode_batch(&batch)?;
    super::admit::closed(store, &rows)?;
    let (held, bytes): (i64, i64) = db(store.conn.query_row(
        "SELECT coalesce(sum(objects), 0), coalesce(sum(length(body)), 0) FROM peer_inbox WHERE sender = ?1",
        params![&sender[..]],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    if held + n > MAX_OBJECTS || bytes + body.len() as i64 > MAX_BYTES {
        return Err(ErrorCode::PeerLimit);
    }
    db(store.conn.execute(
        "INSERT INTO peer_inbox (sender, body, objects, received_at) VALUES (?1, ?2, ?3, ?4)",
        params![&sender[..], body, n, now as i64],
    ))?;
    Ok(n as u64)
}

/// A revoked or pending-revocation peer's held puts are dropped.
pub fn purge(store: &VaultStore, sender: &[u8; 16]) -> Result<(), ErrorCode> {
    db(store.conn.execute("DELETE FROM peer_inbox WHERE sender = ?1", params![&sender[..]])).map(|_| ())
}

/// At unlock: admit every held put whose sender may still speak, oldest
/// first; anything else is dropped. Returns how many were admitted.
pub fn drain(store: &mut VaultStore, vk: &SecretBytes<32>) -> Result<u64, ErrorCode> {
    let held: Vec<(i64, Vec<u8>, Vec<u8>)> = {
        let mut stmt = db(store.conn.prepare("SELECT id, sender, body FROM peer_inbox ORDER BY id"))?;
        let rows = db(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))))?;
        rows.collect::<Result<_, _>>().map_err(|_| ErrorCode::DbCorrupt)?
    };
    if held.is_empty() {
        return Ok(0);
    }
    let reg = crate::registry::log::read_state(&store.dir, &store.header.vault_id.0, &crate::registry::chain::EpochPolicy::CheckpointAnchored)?;
    let targets = crate::sync::pending::revocation_targets(&store.conn)?;
    let mut admitted = 0;
    for (id, sender, body) in held {
        db(store.conn.execute("DELETE FROM peer_inbox WHERE id = ?1", params![id]))?;
        let Ok(sender) = <[u8; 16]>::try_from(sender.as_slice()) else { continue };
        if reg.active_device(&sender).is_none() || targets.contains(&sender) {
            continue;
        }
        if let Ok(c) = super::admit::put(store, vk, sender, &body) {
            admitted += c.admitted;
        }
    }
    Ok(admitted)
}
