//! Peer exchange bodies (wire annex revision 3, A.3.2 – A.3.5):
//! `peer_state` (state and objects modes), `peer_heads`,
//! `peer_revs_get` / `peer_revs_put`. Unavailable items are listed with a
//! reason, never dropped, so `complete = 0` only ever means "truncated by
//! a cap — ask again".

use super::body::{concat, entry, ids_from, uint, Doc, MAX_HEADS};
use crate::crypto::tlv::encode_document;
use crate::errors::ErrorCode;

fn bad() -> ErrorCode {
    ErrorCode::FormatInvalid
}

fn check_roundtrip(encoded: Vec<u8>, bytes: &[u8]) -> Result<(), ErrorCode> {
    if encoded == bytes { Ok(()) } else { Err(bad()) }
}

/// A.3.2 request: state mode or objects mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateReq {
    State { have_generation: u64 },
    /// `(sha256, offset)`, ascending by `sha256`, ≤ 64.
    Objects { state_commit: [u8; 32], wants: Vec<([u8; 32], u64)> },
}

impl StateReq {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            StateReq::State { have_generation } => encode_document(&[entry(&[(0x01, uint(*have_generation))])]),
            StateReq::Objects { state_commit, wants } => {
                let mut es = vec![entry(&[(0x02, state_commit.to_vec())])];
                es.extend(wants.iter().map(|(h, o)| entry(&[(0x01, h.to_vec()), (0x02, uint(*o))])));
                encode_document(&es)
            }
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<StateReq, ErrorCode> {
        let d = Doc::parse(bytes)?;
        let head = d.entry(0, &[1, 2])?;
        let req = if head.has(0x01) && !head.has(0x02) && d.len() == 1 {
            StateReq::State { have_generation: head.uint(0x01)? }
        } else if head.has(0x02) && !head.has(0x01) && (2..=65).contains(&d.len()) {
            let mut wants = Vec::new();
            for i in 1..d.len() {
                let e = d.entry(i, &[1, 2])?;
                wants.push((e.fixed(0x01)?, e.uint(0x02)?));
            }
            if wants.windows(2).any(|w| w[0].0 >= w[1].0) {
                return Err(bad());
            }
            StateReq::Objects { state_commit: head.fixed(0x02)?, wants }
        } else {
            return Err(bad());
        };
        check_roundtrip(req.encode(), bytes)?;
        Ok(req)
    }
}

/// One objects-mode item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectItem {
    Chunk { sha256: [u8; 32], offset: u64, total_len: u64, bytes: Vec<u8> },
    Unavailable { sha256: [u8; 32], reason: u8 },
}

/// A.3.2 objects-mode response (state mode answers `{0x01 state}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Objects {
    pub complete: bool,
    pub items: Vec<ObjectItem>,
}

impl Objects {
    pub fn encode(&self) -> Vec<u8> {
        let mut es = vec![entry(&[(0x01, uint(u64::from(self.complete)))])];
        for it in &self.items {
            es.push(match it {
                ObjectItem::Chunk { sha256, offset, total_len, bytes } => {
                    entry(&[(0x01, sha256.to_vec()), (0x02, uint(*offset)), (0x03, uint(*total_len)), (0x04, bytes.clone())])
                }
                ObjectItem::Unavailable { sha256, reason } => entry(&[(0x01, sha256.to_vec()), (0x05, vec![*reason])]),
            });
        }
        encode_document(&es)
    }
}

impl Objects {
    /// Strict: a flag header, then chunks `{sha256, offset, total_len,
    /// bytes}` that lie inside their object, or unavailable `{sha256,
    /// reason 1–4}` entries.
    pub fn decode(bytes: &[u8]) -> Result<Objects, ErrorCode> {
        let d = Doc::parse(bytes)?;
        let complete = d.entry(0, &[1])?.flag(0x01)?;
        let mut items = Vec::new();
        for i in 1..d.len() {
            let e = d.entry(i, &[1, 2, 3, 4, 5])?;
            let sha256 = e.fixed(0x01)?;
            items.push(match (e.has(0x02), e.has(0x03), e.opt(0x04), e.opt(0x05)) {
                (true, true, Some(b), None) => {
                    let (offset, total_len) = (e.uint(0x02)?, e.uint(0x03)?);
                    if offset.checked_add(b.len() as u64).is_none_or(|end| end > total_len) {
                        return Err(bad());
                    }
                    ObjectItem::Chunk { sha256, offset, total_len, bytes: b.to_vec() }
                }
                (false, false, None, Some([r])) if (1..=4).contains(r) => ObjectItem::Unavailable { sha256, reason: *r },
                _ => return Err(bad()),
            });
        }
        let o = Objects { complete, items };
        check_roundtrip(o.encode(), bytes)?;
        Ok(o)
    }
}

/// `{0x01 state}`: the verified state re-encoded from its fields (annex
/// A.3.2, `sync::remote::canonical`).
pub fn encode_state(state: &[u8]) -> Vec<u8> {
    encode_document(&[entry(&[(0x01, state.to_vec())])])
}

/// A.3.3 request: whole buckets, one byte each, ascending, distinct.
pub fn encode_heads_req(buckets: &[u8]) -> Vec<u8> {
    encode_document(&[entry(&[(0x01, buckets.to_vec())])])
}

pub fn decode_heads_req(bytes: &[u8]) -> Result<Vec<u8>, ErrorCode> {
    let d = Doc::parse(bytes)?;
    if d.len() != 1 {
        return Err(bad());
    }
    let b = d.entry(0, &[1])?.bytes(0x01)?.to_vec();
    if b.windows(2).any(|w| w[0] >= w[1]) {
        return Err(bad());
    }
    check_roundtrip(encode_heads_req(&b), bytes)?;
    Ok(b)
}

/// One record in a `peer_heads` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadsItem {
    Heads { record_id: [u8; 16], heads: Vec<[u8; 32]> },
    Unavailable { record_id: [u8; 16], reason: u8 },
}

/// A.3.3 response: `buckets` lists the buckets fully covered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadsResp {
    pub complete: bool,
    pub buckets: Vec<u8>,
    pub items: Vec<HeadsItem>,
}

impl HeadsResp {
    pub fn encode(&self) -> Vec<u8> {
        let mut head = vec![(0x01, uint(u64::from(self.complete)))];
        if !self.buckets.is_empty() {
            head.push((0x02, self.buckets.clone()));
        }
        let mut es = vec![entry(&head)];
        for it in &self.items {
            es.push(match it {
                HeadsItem::Heads { record_id, heads } => entry(&[(0x01, record_id.to_vec()), (0x02, concat(heads))]),
                HeadsItem::Unavailable { record_id, reason } => entry(&[(0x01, record_id.to_vec()), (0x05, vec![*reason])]),
            });
        }
        encode_document(&es)
    }

    pub fn decode(bytes: &[u8]) -> Result<HeadsResp, ErrorCode> {
        let d = Doc::parse(bytes)?;
        let head = d.entry(0, &[1, 2])?;
        let mut items = Vec::new();
        for i in 1..d.len() {
            let e = d.entry(i, &[1, 2, 5])?;
            let record_id = e.fixed(0x01)?;
            items.push(match (e.opt(0x02), e.opt(0x05)) {
                (Some(h), None) => HeadsItem::Heads { record_id, heads: ids_from(h, MAX_HEADS)? },
                (None, Some([r])) => HeadsItem::Unavailable { record_id, reason: *r },
                _ => return Err(bad()),
            });
        }
        let r = HeadsResp { complete: head.flag(0x01)?, buckets: head.opt(0x02).map(<[u8]>::to_vec).unwrap_or_default(), items };
        // Annex A.3.3: covered buckets ascend; each item is in one of them,
        // bucket by bucket, records ascending within a bucket; reasons 1–4.
        if r.buckets.windows(2).any(|w| w[0] >= w[1]) {
            return Err(bad());
        }
        let mut last: Option<(u8, [u8; 16])> = None;
        for it in &r.items {
            let (rid, reason) = match it {
                HeadsItem::Heads { record_id, .. } => (*record_id, None),
                HeadsItem::Unavailable { record_id, reason } => (*record_id, Some(*reason)),
            };
            let key = (super::body::bucket(&rid), rid);
            if !r.buckets.contains(&key.0) || last.is_some_and(|l| l >= key) || reason.is_some_and(|x| !(1..=4).contains(&x)) {
                return Err(bad());
            }
            last = Some(key);
        }
        check_roundtrip(r.encode(), bytes)?;
        Ok(r)
    }
}

/// A.3.4 request: per record the requester's heads (≤ 512, ascending).
pub fn encode_revs_get(wants: &[([u8; 16], Vec<[u8; 32]>)]) -> Vec<u8> {
    let mut es = vec![entry(&[])];
    for (id, have) in wants {
        let mut f = vec![(0x01, id.to_vec())];
        if !have.is_empty() {
            f.push((0x02, concat(have)));
        }
        es.push(entry(&f));
    }
    encode_document(&es)
}

pub fn decode_revs_get(bytes: &[u8]) -> Result<Vec<([u8; 16], Vec<[u8; 32]>)>, ErrorCode> {
    let d = Doc::parse(bytes)?;
    if d.len() > 513 {
        return Err(bad());
    }
    d.entry(0, &[])?;
    let mut out: Vec<([u8; 16], Vec<[u8; 32]>)> = Vec::new();
    for i in 1..d.len() {
        let e = d.entry(i, &[1, 2])?;
        let have = match e.opt(0x02) {
            Some(b) => ids_from(b, MAX_HEADS)?,
            None => Vec::new(),
        };
        out.push((e.fixed(0x01)?, have));
    }
    if out.windows(2).any(|w| w[0].0 >= w[1].0) {
        return Err(bad());
    }
    check_roundtrip(encode_revs_get(&out), bytes)?;
    Ok(out)
}

/// A.3.4 response / A.3.5 request: objects in canonical order, then any
/// unavailable records (ascending `record_id`). A put has an empty header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revs {
    /// `None` for a put (empty header); `Some(complete)` for a get answer.
    pub complete: Option<bool>,
    pub objects: Vec<Vec<u8>>,
    pub unavailable: Vec<([u8; 16], u8)>,
}

impl Revs {
    pub fn encode(&self) -> Vec<u8> {
        let head = match self.complete {
            Some(c) => entry(&[(0x01, uint(u64::from(c)))]),
            None => entry(&[]),
        };
        let mut es = vec![head];
        es.extend(self.objects.iter().map(|o| entry(&[(0x01, o.clone())])));
        es.extend(self.unavailable.iter().map(|(id, r)| entry(&[(0x02, id.to_vec()), (0x05, vec![*r])])));
        encode_document(&es)
    }

    pub fn decode(bytes: &[u8], put: bool) -> Result<Revs, ErrorCode> {
        let d = Doc::parse(bytes)?;
        let complete = if put {
            d.entry(0, &[])?;
            None
        } else {
            Some(d.entry(0, &[1])?.flag(0x01)?)
        };
        let (mut objects, mut unavailable) = (Vec::new(), Vec::new());
        for i in 1..d.len() {
            let e = d.entry(i, &[1, 2, 5])?;
            match (e.opt(0x01), e.opt(0x02), e.opt(0x05)) {
                (Some(o), None, None) if unavailable.is_empty() => objects.push(o.to_vec()),
                (None, Some(id), Some([r])) if !put => unavailable.push((id.try_into().map_err(|_| bad())?, *r)),
                _ => return Err(bad()),
            }
        }
        // Unavailable entries follow all objects, ascending (annex A.3.4).
        if unavailable.windows(2).any(|w| w[0].0 >= w[1].0) {
            return Err(bad());
        }
        let r = Revs { complete, objects, unavailable };
        check_roundtrip(r.encode(), bytes)?;
        Ok(r)
    }
}
