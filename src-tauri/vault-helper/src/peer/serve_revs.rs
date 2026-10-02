//! `peer_heads` and `peer_revs_get` (wire annex A.3.3, A.3.4): whole
//! buckets, unavailable entries with reasons, closures never split, the
//! canonical order, and the caps (8 MiB, 2,000 revisions per response).

use vault_proto::backup::object;
use vault_proto::peer::body::{bucket, MAX_HEADS, TOO_LARGE, TOO_MANY_HEADS, WITHHELD};
use vault_proto::peer::exchange::{HeadsItem, HeadsResp, Revs};

use super::graph::{closure, heads, Servable};
use crate::errors::ErrorCode;

pub const MAX_RESPONSE: usize = 8 << 20;
pub const MAX_REVS: usize = 2_000;
/// Room kept for the Document wrapper and the header entry.
const OVERHEAD: usize = 64;

/// Whole buckets in the requested order, until the next does not fit.
pub fn heads_for(s: &Servable, buckets: &[u8]) -> HeadsResp {
    let mut resp = HeadsResp { complete: true, buckets: Vec::new(), items: Vec::new() };
    let mut size = OVERHEAD;
    for b in buckets {
        let mut items = Vec::new();
        let mut cost = 0;
        for (rid, rows) in s.records.iter().filter(|(rid, _)| bucket(rid) == *b) {
            let h = heads(rows);
            let item = if h.len() > MAX_HEADS {
                HeadsItem::Unavailable { record_id: *rid, reason: TOO_MANY_HEADS }
            } else {
                HeadsItem::Heads { record_id: *rid, heads: h }
            };
            cost += match &item {
                HeadsItem::Heads { heads, .. } => 16 + heads.len() * 32 + 16,
                HeadsItem::Unavailable { .. } => 32,
            };
            items.push(item);
        }
        if size + cost > MAX_RESPONSE {
            resp.complete = false;
            break;
        }
        size += cost;
        resp.buckets.push(*b);
        resp.items.extend(items);
    }
    resp
}

/// Each wanted record's closure, never split; the first that does not fit
/// stops the page (`complete = 0`); one that never fits is unavailable.
pub fn revs_for(s: &Servable, wants: &[([u8; 16], Vec<[u8; 32]>)]) -> Result<Revs, ErrorCode> {
    let mut out = Revs { complete: Some(true), objects: Vec::new(), unavailable: Vec::new() };
    let (mut size, mut count) = (OVERHEAD, 0usize);
    for (rid, have) in wants {
        let Some(rows) = s.records.get(rid) else {
            if s.withheld.contains(rid) {
                out.unavailable.push((*rid, WITHHELD));
            }
            continue;
        };
        let objs: Vec<Vec<u8>> = closure(rows, have).iter().map(object::encode).collect::<Result<_, _>>()?;
        let cost: usize = objs.iter().map(|o| o.len() + 10).sum();
        if objs.len() > MAX_REVS || cost + OVERHEAD > MAX_RESPONSE {
            out.unavailable.push((*rid, TOO_LARGE));
            continue;
        }
        if count + objs.len() > MAX_REVS || size + cost > MAX_RESPONSE {
            out.complete = Some(false);
            break;
        }
        size += cost;
        count += objs.len();
        out.objects.extend(objs);
    }
    Ok(out)
}
