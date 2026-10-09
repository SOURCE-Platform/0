//! XV-PEER, Rust side (wire annex A.5 with the 2026-10-09 errata): every
//! committed exchange decodes strictly, its prehashes and low-S
//! signatures verify under the fixed test keys, bodies hash to
//! `body_sha256` and decode as their operation (both `peer_state` answers
//! included); the carriage entries, heads digest, state commitment and
//! canonical batch recompute; every invalid case is refused by the decoder
//! it names, with the error its expected outcome implies. Synthetic data.

use serde_json::Value;
use vault_helper::crypto::hex;
use vault_helper::errors::ErrorCode;
use vault_helper::peer::admit::decode_batch;
use vault_proto::crypto::recovery_auth::RecoveryClass;
use vault_proto::crypto::tlv::{decode_document, EntryReader};
use vault_proto::peer::body::{self, heads_digest, Doc, Hello, PutCounts, Status};
use vault_proto::peer::exchange::{decode_heads_req, decode_revs_get, HeadsResp, Objects, Revs, StateReq};
use vault_proto::peer::{body_hash, status_body_ok, verify, PeerOp, PeerRequest, PeerResponse, PeerStatus};
use vault_proto::state::{recovery_auth_digest, state_commit, RecoveryAuthEntry};

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

/// A.3.2 state mode: one entry carrying the state JSON, re-encoded by the
/// shipped encoder, its commitment recomputed from the fields.
fn state_body(f: &Value, b: &[u8]) {
    let d = Doc::parse(b).unwrap();
    assert_eq!(d.len(), 1);
    let json = d.entry(0, &[1]).unwrap().bytes(0x01).unwrap().to_vec();
    let v: Value = serde_json::from_slice(&json).unwrap();
    let b64 = |k: &str| vault_proto::b64::decode(v[k].as_str().unwrap()).expect("base64url, unpadded");
    let auth: Vec<RecoveryAuthEntry> = v["recovery_auth"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| RecoveryAuthEntry {
            class: match e["class"].as_u64() { Some(2) => RecoveryClass::Mp, Some(3) => RecoveryClass::Rk, c => panic!("class {c:?}") },
            public: arr(&e["pub"]),
            salt: arr(&e["salt"]),
        })
        .collect();
    assert_eq!(auth.len(), 2, "an mp and an rk entry");
    let commit = arr::<32>(&v["state_commit"]);
    let gen = v["generation"].as_u64().unwrap();
    let digest = recovery_auth_digest(&auth).unwrap();
    assert_eq!(digest, arr::<32>(&f["state"]["recovery_auth_digest"]));
    let vault_id = arr::<16>(&f["state"]["vault_id"]);
    assert_eq!(state_commit(&vault_id, gen, &body_hash(&b64("manifest")), &body_hash(&b64("checkpoint")), &digest), commit);
    let vk_gen = v["vk_generation"].as_u64().unwrap() as u32;
    let reencoded = vault_helper::sync::remote::encode_fields(gen, vk_gen, &commit, &b64("manifest"), &b64("checkpoint"), &auth);
    assert_eq!(reencoded, json, "the committed bytes are the shipped encoder's");
}

/// The body decodes as its operation (requests and responses differ).
fn body_decodes(f: &Value, op: PeerOp, request: bool, b: &[u8], status: PeerStatus) {
    assert!(status_body_ok(status, b), "status 1–4 carries the empty body");
    if status != PeerStatus::Ok {
        return;
    }
    let empty = b == body::empty().as_slice();
    let ok = match (op, request) {
        (PeerOp::Hello | PeerOp::Status, true) => empty,
        (PeerOp::Hello, false) => Hello::decode(b).is_ok(),
        (PeerOp::State, true) => StateReq::decode(b).is_ok(),
        (PeerOp::State, false) => {
            match Objects::decode(b) {
                Ok(o) => assert!(!o.complete && o.items.len() == 2, "a truncated page"),
                Err(_) => state_body(f, b),
            }
            true
        }
        (PeerOp::Heads, true) => decode_heads_req(b).is_ok(),
        (PeerOp::Heads, false) => HeadsResp::decode(b).is_ok(),
        (PeerOp::RevsGet, true) => decode_revs_get(b).is_ok(),
        (PeerOp::RevsGet, false) => Revs::decode(b, false).and_then(|r| decode_batch(&r)).is_ok(),
        (PeerOp::RevsPut, true) => Revs::decode(b, true).is_ok(),
        (PeerOp::RevsPut, false) => PutCounts::decode(b).is_ok(),
        (PeerOp::Status, false) => Status::decode(b).is_ok(),
        (PeerOp::Unknown(_), _) => empty,
    };
    assert!(ok, "{op:?} {} body decodes", if request { "request" } else { "response" });
}

fn carriage_matches(part: &Value) {
    let c = bytes(&part["carriage"]);
    let r = EntryReader::parse(&c).unwrap();
    assert_eq!(r.tags().collect::<Vec<_>>(), vec![1, 2, 3]);
    for (t, k) in [(1, "tlv"), (2, "signature"), (3, "body")] {
        assert_eq!(r.get(t).unwrap(), bytes(&part[k]).as_slice());
    }
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
        assert_eq!(u64::from(resp.status.code()), x["status"].as_u64().unwrap());
        carriage_matches(q);
        carriage_matches(a);
        body_decodes(&f, req.operation, true, &bytes(&q["body"]), PeerStatus::Ok);
        body_decodes(&f, req.operation, false, &bytes(&a["body"]), resp.status);
    }
}

#[test]
fn the_empty_body_and_zero_are_fixed() {
    let f = file();
    assert_eq!(bytes(&f["empty_body"]["hex"]), vec![0x00, 0, 0, 0, 1, 0xFF]);
    assert_eq!(body_hash(&bytes(&f["empty_body"]["hex"])), arr::<32>(&f["empty_body"]["sha256"]));
    assert_eq!(bytes(&f["zero_integer"]), vec![0x00]);
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
    assert!(records.iter().any(|r| r.1.len() == 2) && records.iter().any(|r| r.1.len() > 64));
    let digest = heads_digest(&records);
    assert_eq!(digest, bytes(&d["digest"]));
    let (b, e) = (d["bucket"].as_u64().unwrap() as usize, d["empty_bucket"].as_u64().unwrap() as usize);
    assert!(records.iter().all(|r| body::bucket(&r.0) as usize == b), "the records share a bucket");
    assert_eq!(&digest[e * 32..e * 32 + 32], bytes(&d["empty_bucket_digest"]).as_slice());
}

#[test]
fn the_three_orders_differ() {
    let b = &file()["revs_batch"];
    let ids = |k: &str| b[k].as_array().unwrap().iter().map(|r| r["revision_id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let (c, q, d) = (ids("canonical_record_1"), ids("fifo_record_1"), ids("depth_first_record_1"));
    assert!(c != q && c != d && q != d);
    let mut sorted = c.clone();
    sorted.sort();
    assert_ne!(c, sorted, "not plain id order either");
}

#[test]
fn every_invalid_case_is_refused_by_its_decoder() {
    let cases = file()["invalid"].as_array().unwrap().clone();
    assert!(cases.len() >= 26, "one per A.1 rule");
    for c in cases {
        let b = bytes(&c["hex"]);
        let expected = c["expected"].as_str().unwrap();
        let err = match c["decoder"].as_str().unwrap() {
            "request_tlv" => PeerRequest::decode(&b).err(),
            "heads_req" => decode_heads_req(&b).err(),
            "document" => decode_document(&b).err().map(|_| ErrorCode::FormatInvalid),
            "put_counts" => PutCounts::decode(&b).err(),
            "revs_get_req" => decode_revs_get(&b).err(),
            "state_req" => StateReq::decode(&b).err(),
            "heads_resp" => HeadsResp::decode(&b).err(),
            "objects_resp" => Objects::decode(&b).err(),
            "revs_batch" => Revs::decode(&b, false).and_then(|r| decode_batch(&r)).err(),
            "status4_body" => (!status_body_ok(PeerStatus::FormatInvalid, &b)).then_some(ErrorCode::FormatInvalid),
            other => panic!("unknown decoder {other}"),
        };
        let want = if expected.starts_with("unsigned 403") { ErrorCode::PeerAuthInvalid } else { ErrorCode::FormatInvalid };
        assert_eq!(err, Some(want), "{} must be refused ({expected})", c["rule"]);
    }
}

#[test]
fn the_extra_valid_bodies_decode() {
    for c in file()["valid_extra"].as_array().unwrap() {
        assert_eq!(c["decoder"], "heads_resp");
        HeadsResp::decode(&bytes(&c["hex"])).unwrap_or_else(|e| panic!("{}: {e:?}", c["rule"]));
    }
}
