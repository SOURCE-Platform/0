//! Operation handlers (wire annex A.3). First: `peer_status` (§22.9) —
//! answered for any key the registry ever installed, in every serving
//! state. The exchange operations follow.

use vault_proto::peer::body::Status;
use vault_proto::peer::PeerStatus;

use super::respond::{sign, Signed};
use super::verify::Accepted;
use super::{Ctx, Refusal};
use crate::storage::VaultStore;

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
