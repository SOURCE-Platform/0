//! XV-PEER, Rust side (wire annex A.5): every committed exchange decodes
//! strictly, its prehashes and low-S signatures verify under the fixed
//! test keys, bodies hash to `body_sha256` and decode as their operation;
//! the heads digest and the canonical batch recompute; every invalid case
//! is refused by the decoder it names. Synthetic data only.

use serde_json::Value;
use vault_helper::crypto::hex;
use vault_helper::peer::admit::decode_batch;
use vault_proto::crypto::tlv::EntryReader;
use vault_proto::peer::body::{heads_digest, Hello, PutCounts, Status};
use vault_proto::peer::exchange::{decode_heads_req, decode_revs_get, HeadsResp, Revs, StateReq};
use vault_proto::peer::{body_hash, verify, PeerOp, PeerRequest, PeerResponse};

fn file() -> Value {
    let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/xv_peer.json")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn bytes(v: &Value) -> Vec<u8> {
    hex::decode(v.as_str().expect("hex string")).expect("hex")
}

fn arr<const N: usize>(v: &Value) -> [u8; N] {
    bytes(v).try_into().expect("width")
}

/// The body decodes as its operation (requests and responses differ).
fn body_decodes(op: PeerOp, request: bool, b: &[u8], status: u64) {
    if status != 0 {
        assert_eq!(b, vault_proto::peer::body::empty(), "status 1–4 carries the empty body");
        return;
    }
    let ok = match (op, request) {
        (PeerOp::Hello, _) => Hello::decode(b).is_ok(),
        (PeerOp::State, true) => StateReq::decode(b).is_ok(),
        (PeerOp::State, false) => b.first() == Some(&0x00),
        (PeerOp::Heads, true) => decode_heads_req(b).is_ok(),
        (PeerOp::Heads, false) => HeadsResp::decode(b).is_ok(),
        (PeerOp::RevsGet, true) => decode_revs_get(b).is_ok(),
        (PeerOp::RevsGet, false) => Revs::decode(b, false).and_then(|r| decode_batch(&r)).is_ok(),
        (PeerOp::RevsPut, true) => Revs::decode(b, true).is_ok(),
        (PeerOp::RevsPut, false) => PutCounts::decode(b).is_ok(),
        (PeerOp::Status, true) => b == vault_proto::peer::body::empty(),
        (PeerOp::Status, false) => Status::decode(b).is_ok(),
        (PeerOp::Unknown(_), _) => b == vault_proto::peer::body::empty(),
    };
    assert!(ok, "{op:?} {} body decodes", if request { "request" } else { "response" });
}

#[test]
fn every_exchange_verifies_and_decodes() {
    let f = file();
    let (phone, mac) = (arr::<65>(&f["keys"]["phone_pub"]), arr::<65>(&f["keys"]["mac_pub"]));
    let exchanges = f["exchanges"].as_array().unwrap();
    let ops: std::collections::BTreeSet<u64> = exchanges.iter().map(|x| x["operation"].as_u64().unwrap()).collect();
    assert_eq!(ops.into_iter().collect::<Vec<_>>(), vec![1, 2, 3, 4, 5, 6, 9], "every operation, plus an unknown one");
    for x in exchanges {
        let (q, a) = (&x["request"], &x["response"]);
        let req = PeerRequest::decode(&bytes(&q["tlv"])).unwrap();
        assert_eq!(req.prehash(), arr::<32>(&q["prehash"]));
        verify(&req.prehash(), &bytes(&q["signature"]), &phone).unwrap();
        assert!(verify(&req.prehash(), &bytes(&q["signature"]), &mac).is_err());
        assert_eq!(body_hash(&bytes(&q["body"])), req.body_sha256);
        let resp = PeerResponse::decode(&bytes(&a["tlv"])).unwrap();
        assert_eq!(resp.request_prehash, req.prehash(), "{}", x["name"]);
        assert_eq!(resp.prehash(), arr::<32>(&a["prehash"]));
        verify(&resp.prehash(), &bytes(&a["signature"]), &mac).unwrap();
        assert_eq!(body_hash(&bytes(&a["body"])), resp.body_sha256);
        let status = x["status"].as_u64().unwrap();
        assert_eq!(u64::from(resp.status.code()), status);
        body_decodes(req.operation, true, &bytes(&q["body"]), 0);
        body_decodes(req.operation, false, &bytes(&a["body"]), status);
    }
}

#[test]
fn the_empty_body_zero_and_carriage_are_fixed() {
    let f = file();
    assert_eq!(bytes(&f["empty_body"]["hex"]), vec![0x00, 0, 0, 0, 1, 0xFF]);
    assert_eq!(body_hash(&bytes(&f["empty_body"]["hex"])), arr::<32>(&f["empty_body"]["sha256"]));
    assert_eq!(bytes(&f["zero_integer"]), vec![0x00]);
    let c = bytes(&f["carriage"]["hex"]);
    let r = EntryReader::parse(&c).unwrap();
    assert_eq!(r.tags().collect::<Vec<_>>(), vec![1, 2, 3]);
    let hello = &f["exchanges"][0]["request"];
    assert_eq!(r.get(1).unwrap(), bytes(&hello["tlv"]).as_slice());
    assert_eq!(r.get(2).unwrap(), bytes(&hello["signature"]).as_slice());
    assert_eq!(r.get(3).unwrap(), bytes(&hello["body"]).as_slice());
}

#[test]
fn the_heads_digest_recomputes() {
    let d = &file()["heads_digest"];
    let records: Vec<([u8; 16], Vec<[u8; 32]>)> = d["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (arr(&r["record_id"]), r["heads"].as_array().unwrap().iter().map(arr::<32>).collect()))
        .collect();
    assert!(records.iter().any(|r| r.1.len() == 2), "one record with two heads");
    let digest = heads_digest(&records);
    assert_eq!(digest, bytes(&d["digest"]));
    let (b, e) = (d["bucket"].as_u64().unwrap() as usize, d["empty_bucket"].as_u64().unwrap() as usize);
    assert!(records.iter().all(|r| vault_proto::peer::body::bucket(&r.0) as usize == b), "both records share a bucket");
    assert_eq!(&digest[e * 32..e * 32 + 32], bytes(&d["empty_bucket_digest"]).as_slice());
}

#[test]
fn every_invalid_case_is_refused_by_its_decoder() {
    for c in file()["invalid"].as_array().unwrap() {
        let b = bytes(&c["hex"]);
        let refused = match c["decoder"].as_str().unwrap() {
            "request_tlv" => PeerRequest::decode(&b).is_err(),
            "heads_req" => decode_heads_req(&b).is_err(),
            "put_counts" => PutCounts::decode(&b).is_err(),
            "revs_get_req" => decode_revs_get(&b).is_err(),
            "state_req" => StateReq::decode(&b).is_err(),
            "empty_body" => vault_proto::crypto::tlv::decode_document(&b).is_err(),
            "revs_batch" => Revs::decode(&b, false).and_then(|r| decode_batch(&r)).is_err(),
            other => panic!("unknown decoder {other}"),
        };
        assert!(refused, "{} must be refused", c["rule"]);
    }
}
