//! What the phone pushes with `peer_revs_put` (spec v0.5 §22.7, wire annex
//! A.3.5; F.2d step 3): for each record in the buckets the exchange
//! covered, the phone's revisions the Mac does not hold — the closure
//! above the Mac's heads, in the canonical order. **Freshness:** local-only
//! revisions (no `provider` source) are pushed only within 15 minutes of
//! the phone's own verified provider exchange; otherwise only
//! provider-confirmed ones (whose ancestors are all provider-confirmed, so
//! the batch stays closed). Never a set-aside revision. Caps: 2,000
//! revisions and the 1 MiB request (an object that would not fit waits
//! for the provider path); whole closures only, and one that does not fit
//! never holds back the records after it.

use std::collections::BTreeMap;

use vault_proto::backup::object;
use vault_proto::peer::body::bucket;
use vault_proto::peer::exchange::Revs;

use crate::errors::ErrorCode;
use crate::storage::revision_rows::all_rows;
use crate::storage::revisions::{uuid_bytes, RevisionRow};
use crate::storage::sources::{self, Source};
use crate::storage::VaultStore;

/// The request cap less room for the carriage (annex A.3.5).
const MAX_PUT: usize = (1 << 20) - 4096;

/// The `peer_revs_put` body, or `None` when the Mac lacks nothing pushable.
pub fn batch(store: &VaultStore, covered: &[u8], theirs: &BTreeMap<[u8; 16], Vec<[u8; 32]>>, fresh: bool) -> Result<Option<Vec<u8>>, ErrorCode> {
    let mut by: BTreeMap<[u8; 16], Vec<RevisionRow>> = BTreeMap::new();
    for row in all_rows(&store.conn)? {
        let rid = uuid_bytes(&row.record_id).ok_or(ErrorCode::DbCorrupt)?;
        if !covered.contains(&bucket(&rid)) || crate::storage::set_aside::refused(&store.conn, &row.revision_id)? {
            continue;
        }
        if !fresh && !sources::of(&store.conn, &row.revision_id)?.contains(&Source::Provider) {
            continue;
        }
        by.entry(rid).or_default().push(row);
    }
    let (mut objects, mut size) = (Vec::new(), 64usize);
    for (rid, rows) in by {
        let heads = theirs.get(&rid).cloned().unwrap_or_default();
        let send = crate::peer::graph::closure(&rows, &heads);
        if send.is_empty() {
            continue;
        }
        let objs: Vec<Vec<u8>> = send.iter().map(object::encode).collect::<Result<_, _>>()?;
        let cost: usize = objs.iter().map(|o| o.len() + 16).sum();
        if objects.len() + objs.len() > crate::peer::admit::MAX_BATCH || size + cost > MAX_PUT {
            continue; // whole closures only; this one waits, smaller ones still go (reviews VER-O3 / SEC-O4)
        }
        size += cost;
        objects.extend(objs);
    }
    Ok((!objects.is_empty()).then(|| Revs { complete: None, objects, unavailable: Vec::new() }.encode()))
}
