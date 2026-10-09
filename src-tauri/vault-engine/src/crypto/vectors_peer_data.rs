//! XV-PEER inputs (wire annex A.5): the synthetic records, the revision
//! graph whose canonical order differs from depth-first and from
//! first-in-first-out order, and one invalid case per A.1 rule with its
//! expected outcome. `vectors_peer` assembles the file.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use vault_proto::backup::object;
use vault_proto::crypto::tlv::{encode_document, EntryBuilder};
use vault_proto::peer::body::{bucket, entry, uint};
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

/// Record 1's graph (review VER-I2): A(10) → B(20), D(40); B → E(50);
/// D → C(30), a child with a smaller id than its parent. Kahn with the
/// smallest ready id: A, B, D, C, E. First-in-first-out: A, B, D, E, C.
/// Depth-first (children ascending): A, B, E, D, C. Record 2: Q(0x60)
/// on P(0x5F), which the requester already holds.
pub struct Graph {
    pub canonical: Vec<RevisionRow>,
    pub fifo: Vec<RevisionRow>,
    pub depth_first: Vec<RevisionRow>,
    pub second: Vec<RevisionRow>,
}

pub fn graph(r: &[[u8; 16]; 3]) -> Graph {
    let (a, b, c, d, e) = (rev(0x10), rev(0x20), rev(0x30), rev(0x40), rev(0x50));
    let [ra, rb, rc, rd, re] = [row(r[1], a, &[], 1), row(r[1], b, &[a], 2), row(r[1], c, &[d], 5), row(r[1], d, &[a], 3), row(r[1], e, &[b], 4)];
    Graph {
        canonical: vec![ra.clone(), rb.clone(), rd.clone(), rc.clone(), re.clone()],
        fifo: vec![ra.clone(), rb.clone(), rd.clone(), re.clone(), rc.clone()],
        depth_first: vec![ra, rb, re, rd, rc],
        second: vec![row(r[2], rev(0x60), &[rev(0x5F)], 6)],
    }
}

pub fn objects(rows: &[RevisionRow]) -> Vec<Vec<u8>> {
    rows.iter().map(|r| object::encode(r).expect("synthetic row encodes")).collect()
}

/// What the Swift target needs to recompute the order without the engine.
pub fn graph_json(rows: &[RevisionRow]) -> Value {
    Value::Array(
        rows.iter()
            .map(|r| json!({ "revision_id": hex::encode(r.revision_id), "parents": r.parent_ids.iter().map(hex::encode).collect::<Vec<_>>() }))
            .collect(),
    )
}

/// The Mac's servable heads: record 0 with 65 (more than 64: reason 4 in
/// `peer_heads`, still in the digest), record 1 with two, record 2 one.
pub fn heads(r: &[[u8; 16]; 3]) -> Vec<([u8; 16], Vec<[u8; 32]>)> {
    vec![(r[0], (0..65u8).map(|i| rev(0x90 + i)).collect()), (r[1], vec![rev(0x30), rev(0x50)]), (r[2], vec![rev(0x60)])]
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

const UNSIGNED: &str = "unsigned 403 (PEER_AUTH_INVALID)";
const STATUS4: &str = "signed status 4 (FORMAT_INVALID)";
const UNVERIFIED: &str = "requester: unable to verify";

fn case(rule: &str, decoder: &str, expected: &str, bytes: Vec<u8>) -> Value {
    json!({ "rule": rule, "decoder": decoder, "expected": expected, "hex": hex::encode(bytes) })
}

fn req(fields: &[(u8, &[u8])]) -> Vec<u8> {
    raw_entry(fields)
}

fn batch(rows: &[Vec<u8>]) -> Vec<u8> {
    encode_document(&std::iter::once(entry(&[(0x01, uint(1))])).chain(rows.iter().map(|o| entry(&[(0x01, o.clone())]))).collect::<Vec<_>>())
}

/// One invalid case per A.1 rule, naming the decoder that must refuse it
/// and the outcome (erratum to A.1, review SPEC-I7). `good` is a valid
/// `request_tlv`.
pub fn invalid_cases(good: &[u8], r: &[[u8; 16]; 3], g: &Graph) -> Value {
    let (v, s, rc, one, z, n) = ([0xA0u8; 16], [0xB1u8; 16], [0xC2u8; 16], [1u8], [0u8; 32], [1u8; 16]);
    let mut trailing = good.to_vec();
    trailing.push(0x00);
    let mut unknown = good[..good.len() - 1].to_vec();
    unknown.extend_from_slice(&[0x09, 0, 0, 0, 1, 7, 0xFF]);
    let missing = good[..good.len() - (1 + 4 + 16 + 1)].iter().copied().chain([0xFF]).collect();
    let after_objects = encode_document(&[entry(&[(0x01, uint(1))]), entry(&[(0x02, r[0].to_vec()), (0x05, vec![1])]), entry(&[(0x01, objects(&g.second)[0].clone())])]);
    let status4_body = encode_document(&[entry(&[(0x01, uint(1))])]);
    json!([
        case("trailing bytes", "request_tlv", UNSIGNED, trailing),
        case("unknown tag", "request_tlv", UNSIGNED, unknown),
        case("missing required tag", "request_tlv", UNSIGNED, missing),
        case("wrong fixed length", "request_tlv", UNSIGNED, req(&[(1, &one), (2, &v[..15]), (3, &s), (4, &rc), (5, &one), (6, &z), (7, &one), (8, &n)])),
        case("tags not ascending", "request_tlv", UNSIGNED, req(&[(1, &one), (3, &s), (2, &v), (4, &rc), (5, &one), (6, &z), (7, &one), (8, &n)])),
        case("tag repeated", "request_tlv", UNSIGNED, req(&[(1, &one), (1, &one), (2, &v), (3, &s), (4, &rc), (5, &one), (6, &z), (7, &one), (8, &n)])),
        case("padded integer", "request_tlv", UNSIGNED, req(&[(1, &one), (2, &v), (3, &s), (4, &rc), (5, &one), (6, &z), (7, &[0, 1]), (8, &n)])),
        case("empty integer (zero is 0x00, never empty)", "request_tlv", UNSIGNED, req(&[(1, &one), (2, &v), (3, &s), (4, &rc), (5, &one), (6, &z), (7, &[]), (8, &n)])),
        case("envelope wrapped in a Document", "request_tlv", UNSIGNED, encode_document(&[good.to_vec()])),
        case("empty value", "heads_req", STATUS4, encode_document(&[raw_entry(&[(1, &[])])])),
        case("body without its header entry", "heads_req", STATUS4, vec![0x00, 0, 0, 0, 0]),
        case("document length wrong", "document", STATUS4, vec![0x00, 0, 0, 0, 2, 0xFF]),
        case("list out of order (buckets)", "heads_req", STATUS4, encode_document(&[entry(&[(0x01, vec![9, 3])])])),
        case("padded integer in a response body", "put_counts", UNVERIFIED, encode_document(&[raw_entry(&[(1, &[0, 1]), (2, &[0]), (3, &[0])])])),
        case("list out of order (record ids)", "revs_get_req", STATUS4, encode_revs_get(&[(r[1], vec![]), (r[0], vec![])])),
        case("empty list present instead of absent", "revs_get_req", STATUS4, encode_document(&[EntryBuilder::new().build(), raw_entry(&[(1, &r[0]), (2, &[])])])),
        case("absent offset (objects mode)", "state_req", STATUS4, encode_document(&[entry(&[(0x02, vec![0x77; 32])]), entry(&[(0x01, vec![0x01; 32])])])),
        case("flag not 0/1 (complete = 2)", "heads_resp", UNVERIFIED, encode_document(&[entry(&[(0x01, uint(2))])])),
        case("revisions in depth-first order", "revs_batch", UNVERIFIED, batch(&objects(&g.depth_first))),
        case("revisions in first-in-first-out order", "revs_batch", UNVERIFIED, batch(&objects(&g.fifo))),
        case("unavailable entry before an object", "revs_batch", UNVERIFIED, after_objects),
        case("status 1–4 with a non-empty body", "status4_body", UNVERIFIED, status4_body),
        case("unavailable entries descending", "revs_batch", UNVERIFIED, encode_document(&[entry(&[(0x01, uint(1))]), entry(&[(0x02, r[2].to_vec()), (0x05, vec![1])]), entry(&[(0x02, r[0].to_vec()), (0x05, vec![1])])])),
        case("chunk past its object's total_len", "objects_resp", UNVERIFIED, encode_document(&[entry(&[(0x01, uint(1))]), entry(&[(0x01, vec![0x71; 32]), (0x02, uint(0)), (0x03, uint(3)), (0x04, b"12345".to_vec())])])),
        case("records out of order within a bucket", "heads_resp", UNVERIFIED, heads_resp(bucket(&r[0]), &[r[1], r[0]])),
        case("record outside the covered buckets", "heads_resp", UNVERIFIED, heads_resp(bucket(&r[0]).wrapping_add(1), &[r[0]])),
    ])
}

fn heads_resp(covered: u8, records: &[[u8; 16]]) -> Vec<u8> {
    let mut es = vec![entry(&[(0x01, uint(1)), (0x02, vec![covered])])];
    es.extend(records.iter().map(|id| entry(&[(0x01, id.to_vec()), (0x02, rev(0x77).to_vec())])));
    encode_document(&es)
}

/// A valid heads answer covering two non-empty buckets, where the later
/// bucket holds the smaller record id: ascent is per bucket (A.3.3).
pub fn two_bucket_heads() -> Vec<u8> {
    let id = |i: u32| {
        let mut r = [0x5Au8; 16];
        r[12..].copy_from_slice(&i.to_be_bytes());
        r
    };
    let first = id(0);
    let other = (1..).map(id).find(|x| bucket(x) < bucket(&first)).expect("a lower bucket");
    let es = vec![
        entry(&[(0x01, uint(1)), (0x02, vec![bucket(&other), bucket(&first)])]),
        entry(&[(0x01, other.to_vec()), (0x02, rev(0x77).to_vec())]),
        entry(&[(0x01, first.to_vec()), (0x02, rev(0x78).to_vec())]),
    ];
    encode_document(&es)
}

pub fn sha(bytes: &[u8]) -> String {
    hex::encode(<[u8; 32]>::from(Sha256::digest(bytes)))
}
