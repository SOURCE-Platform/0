//! Operation handlers (wire annex A.3) and the state gate in front of
//! them: a behind Mac answers only hello and status (§22.14); COMPROMISED
//! takes no `peer_revs_put`; everything else is answered per A.3.

use vault_proto::peer::body::{heads_digest, Hello, Status};
use vault_proto::peer::exchange::{decode_heads_req, decode_revs_get};
use vault_proto::peer::{PeerOp, PeerStatus};

use super::graph::{heads, servable};
use super::respond::{sign, status_only, Signed};
use super::serve_revs::{heads_for, revs_for};
use super::verify::Accepted;
use super::{Ctx, Refusal};
use crate::crypto::secret::SecretBytes;
use crate::storage::VaultStore;

/// Answer an authenticated request whose body hashed correctly. `vk` is
/// the resident key while UNLOCKED (a put needs it to open revisions).
pub fn serve(ctx: &Ctx, store: &mut VaultStore, vk: Option<&SecretBytes<32>>, acc: &Accepted, body: &[u8], now: u64) -> Result<Signed, Refusal> {
    let op = acc.req.operation;
    let gated = (ctx.behind && !matches!(op, PeerOp::Hello | PeerOp::Status)) || (ctx.compromised && op == PeerOp::RevsPut);
    if gated {
        return status_only(ctx, acc, PeerStatus::BadState, now);
    }
    let answered = match op {
        PeerOp::Status => return status(ctx, store, acc, now),
        PeerOp::Hello => hello(ctx, store).map(|b| (PeerStatus::Ok, b)),
        PeerOp::Heads => match decode_heads_req(body) {
            Ok(buckets) => servable(ctx, store).map(|s| (PeerStatus::Ok, heads_for(&s, &buckets).encode())),
            Err(_) => return status_only(ctx, acc, PeerStatus::FormatInvalid, now),
        },
        PeerOp::RevsGet => match decode_revs_get(body) {
            Ok(wants) => servable(ctx, store).and_then(|s| revs_for(&s, &wants)).map(|r| (PeerStatus::Ok, r.encode())),
            Err(_) => return status_only(ctx, acc, PeerStatus::FormatInvalid, now),
        },
        PeerOp::RevsPut => match vk.filter(|_| !ctx.locked) {
            Some(vk) => match super::admit::put(store, vk, acc.req.sender_device_id, body) {
                Ok(counts) => Ok((PeerStatus::Ok, counts.encode())),
                Err(crate::errors::ErrorCode::FormatInvalid) => return status_only(ctx, acc, PeerStatus::FormatInvalid, now),
                Err(e) => Err(e),
            },
            // The LOCKED inbox (A.3.5) is not built yet.
            None => return status_only(ctx, acc, PeerStatus::BadState, now),
        },
        // Wire annex A.3.2 needs the stored verified state body: not yet.
        PeerOp::State => return status_only(ctx, acc, PeerStatus::BadState, now),
    };
    match answered {
        Ok((st, b)) => sign(ctx, acc, st, b, now),
        Err(_) => Err(Refusal::Unavailable),
    }
}

/// `peer_hello`: the committed registry head, the committed provider
/// state, and the servable heads digest.
fn hello(ctx: &Ctx, store: &VaultStore) -> Result<Vec<u8>, crate::errors::ErrorCode> {
    let reg = crate::sync::fetch::confirmed_registry(store)?;
    let seen = crate::sync::seen::load(&store.conn)?;
    let s = servable(ctx, store)?;
    let records: Vec<([u8; 16], Vec<[u8; 32]>)> = s.records.iter().map(|(id, rows)| (*id, heads(rows))).collect();
    Ok(Hello {
        registry_seq: reg.entries.len().saturating_sub(1) as u64,
        registry_head: reg.head,
        committed_generation: seen.as_ref().map_or(0, |s| s.generation),
        committed_manifest_hash: seen.map_or([0; 32], |s| s.manifest_hash.0),
        heads_digest: heads_digest(&records),
    }
    .encode())
}

/// `peer_status`: the local chain, the seq of the last provider-committed
/// entry, and the committed provider generation and manifest hash.
pub fn status(ctx: &Ctx, store: &VaultStore, accepted: &Accepted, now: u64) -> Result<Signed, Refusal> {
    let registry = std::fs::read(crate::registry::log::path(&ctx.dir)).map_err(|_| Refusal::Unavailable)?;
    let committed = crate::sync::fetch::confirmed_registry(store).map_err(|_| Refusal::Unavailable)?;
    let seen = crate::sync::seen::load(&store.conn).map_err(|_| Refusal::Unavailable)?;
    let body = Status {
        vault_id: ctx.vault_id,
        registry,
        committed_seq: committed.entries.len().saturating_sub(1) as u64,
        committed_generation: seen.as_ref().map_or(0, |s| s.generation),
        committed_manifest_hash: seen.map_or([0; 32], |s| s.manifest_hash.0),
    };
    sign(ctx, accepted, PeerStatus::Ok, body.encode(), now)
}
