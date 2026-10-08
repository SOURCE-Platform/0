//! XV-PEER inputs (wire annex A.5): the synthetic records, the revision
//! graph whose canonical order differs from depth-first order, and one
//! invalid case per A.1 rule. `vectors_peer` assembles the file.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use vault_proto::backup::object;
use vault_proto::crypto::tlv::{encode_document, EntryBuilder};
use vault_proto::peer::body::{self, bucket, entry, uint};
use vault_proto::peer::exchange::encode_revs_get;
use vault_proto::rev::{uuid_string, RevisionRow};

use super::hex;

pub const AUTHOR: [u8; 16] = [0xB1; 16];

/// Three record ids in one bucket (ascending), and a bucket none of them
/// is in — found by counting up from fixed prefixes, so reproducible.
pub fn records() -> ([[u8; 16]; 3], u8) {
    let id = |i: u32| {
        let mut r = [0x5Au8; 16];
        r[12..].copy_from_slice(&i.to_be_bytes());
        r
    };
    let target = bucket(&id(0));
    let mut found = vec![id(0)];
    let mut i = 1;
    while found.len() < 3 {
        if bucket(&id(i)) == target {
            found.push(id(i));
        }
        i += 1;
    }
    let empty = (0..=255u8).find(|b| *b != target).expect("another bucket");
    ([found[0], found[1], found[2]], empty)
}

pub fn rev(b: u8) -> [u8; 32] {
    [b; 32]
}

fn row(record: [u8; 16], id: [u8; 32], parents: &[[u8; 32]], counter: u64) -> RevisionRow {
    RevisionRow {
        revision_id: id,
        record_id: uuid_string(&record),
        parent_ids: parents.to_vec(),
        author_device: uuid_string(&AUTHOR),
        counter,
        deleted: false,
        kind_tag: 1,
        vk_generation: 1,
        schema_version: 1,
        nonce: [0x61; 24],
        ct: b"synthetic-ct".to_vec(),
        meta_nonce: [0x62; 24],
        meta_ct: b"synthetic-meta".to_vec(),
        created_at: 1_790_000_000,
        updated_at: 1_790_000_000 + counter,
    }
}

/// Record 1's graph: A(0x10) → B(0x20), C(0x30); B → E(0x40). Kahn with
/// the smallest ready id gives A, B, C, E; depth-first (children
/// ascending) gives A, B, E, C. Record 2: Q(0x50) on P(0x4F), which the
/// requester already holds.
pub fn graph_rows(r: &[[u8; 16]; 3]) -> (Vec<RevisionRow>, Vec<RevisionRow>, Vec<RevisionRow>) {
    let (a, b, c, e) = (rev(0x10), rev(0x20), rev(0x30), rev(0x40));
    let canonical = vec![row(r[0], a, &[], 1), row(r[0], b, &[a], 2), row(r[0], c, &[a], 3), row(r[0], e, &[b], 4)];
    let depth_first = vec![canonical[0].clone(), canonical[1].clone(), canonical[3].clone(), canonical[2].clone()];
    let second = vec![row(r[1], rev(0x50), &[rev(0x4F)], 6)];
    (canonical, depth_first, second)
}

pub fn objects(rows: &[RevisionRow]) -> Vec<Vec<u8>> {
    rows.iter().map(|r| object::encode(r).expect("synthetic row encodes")).collect()
}

/// What the Swift target needs to recompute the order without the engine.
pub fn graph_json(rows: &[RevisionRow]) -> Value {
    Value::Array(
        rows.iter()
            .map(|r| {
                json!({
                    "revision_id": hex::encode(r.revision_id),
                    "parents": r.parent_ids.iter().map(hex::encode).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

/// The peer heads per record (record 1 with two heads, record 2 with one).
pub fn heads(r: &[[u8; 16]; 3]) -> Vec<([u8; 16], Vec<[u8; 32]>)> {
    vec![(r[0], vec![rev(0x30), rev(0x40)]), (r[1], vec![rev(0x50)])]
}

fn raw_entry(fields: &[(u8, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (t, v) in fields {
        out.push(*t);
        out.extend_from_slice(&(v.len() as u32).to_be_bytes());
        out.extend_from_slice(v);
    }
    out.push(0xFF);
    out
}

fn case(rule: &str, decoder: &str, bytes: Vec<u8>) -> Value {
    json!({ "rule": rule, "decoder": decoder, "hex": hex::encode(bytes) })
}

/// One invalid case per A.1 rule, each naming the decoder that must
/// refuse it. `good` is a valid `request_tlv`.
pub fn invalid_cases(good: &[u8], r: &[[u8; 16]; 3], depth_first: &[Vec<u8>]) -> Value {
    let mut trailing = good.to_vec();
    trailing.push(0x00);
    let mut unknown = good[..good.len() - 1].to_vec();
    unknown.extend_from_slice(&[0x09, 0, 0, 0, 1, 7, 0xFF]);
    let missing = good[..good.len() - (1 + 4 + 16 + 1)].iter().copied().chain([0xFF]).collect();
    let short_vault = raw_entry(&[(1, &[1]), (2, &[0xA0; 15]), (3, &[0xB1; 16]), (4, &[0xC2; 16]), (5, &[1]), (6, &[0; 32]), (7, &[1]), (8, &[1; 16])]);
    let swapped = raw_entry(&[(1, &[1]), (3, &[0xB1; 16]), (2, &[0xA0; 16]), (4, &[0xC2; 16]), (5, &[1]), (6, &[0; 32]), (7, &[1]), (8, &[1; 16])]);
    let duplicate = raw_entry(&[(1, &[1]), (1, &[1]), (2, &[0xA0; 16]), (3, &[0xB1; 16]), (4, &[0xC2; 16]), (5, &[1]), (6, &[0; 32]), (7, &[1]), (8, &[1; 16])]);
    let padded_t = raw_entry(&[(1, &[1]), (2, &[0xA0; 16]), (3, &[0xB1; 16]), (4, &[0xC2; 16]), (5, &[1]), (6, &[0; 32]), (7, &[0, 1]), (8, &[1; 16])]);
    let empty_value = encode_document(&[raw_entry(&[(1, &[])])]);
    let buckets_down = encode_document(&[entry(&[(0x01, vec![9, 3])])]);
    let padded_count = encode_document(&[raw_entry(&[(1, &[0, 1]), (2, &[0]), (3, &[0])])]);
    let unsorted = encode_revs_get(&[(r[1], vec![]), (r[0], vec![])]);
    let empty_list = encode_document(&[EntryBuilder::new().build(), raw_entry(&[(1, &r[0]), (2, &[])])]);
    let absent_offset = encode_document(&[entry(&[(0x02, vec![0x77; 32])]), entry(&[(0x01, vec![0x01; 32])])]);
    let doc_len = {
        let mut d = body::empty();
        d[4] = 2;
        d
    };
    let dfs = encode_document(&std::iter::once(entry(&[(0x01, uint(1))])).chain(depth_first.iter().map(|o| entry(&[(0x01, o.clone())]))).collect::<Vec<_>>());
    json!([
        case("trailing bytes", "request_tlv", trailing),
        case("unknown tag", "request_tlv", unknown),
        case("missing required tag", "request_tlv", missing),
        case("wrong fixed length", "request_tlv", short_vault),
        case("tags not ascending", "request_tlv", swapped),
        case("tag repeated", "request_tlv", duplicate),
        case("padded integer", "request_tlv", padded_t),
        case("empty value", "heads_req", empty_value),
        case("list out of order (buckets)", "heads_req", buckets_down),
        case("padded integer in a body", "put_counts", padded_count),
        case("list out of order (record ids)", "revs_get_req", unsorted),
        case("empty list present instead of absent", "revs_get_req", empty_list),
        case("absent offset (objects mode)", "state_req", absent_offset),
        case("document length wrong", "empty_body", doc_len),
        case("revisions in depth-first, not canonical, order", "revs_batch", dfs),
    ])
}

pub fn sha(bytes: &[u8]) -> String {
    hex::encode(<[u8; 32]>::from(Sha256::digest(bytes)))
}
