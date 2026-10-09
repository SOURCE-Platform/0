//! Peer wire format (spec v0.5 §22.8; wire annex revision 3 A.1–A.3):
//! envelope and body round trips, strict decoding (PW-02), the zero and
//! empty-body encodings, signatures under the peer prefixes, and the
//! heads digest. Synthetic values only.

use vault_proto::crypto::ecdsa::{dev_keypair_from_scalar, dev_sign_prehash};
use vault_proto::crypto::tlv::{encode_document, EntryBuilder};
use vault_proto::errors::ErrorCode;
use vault_proto::peer::body::{self, bucket, heads_digest, Hello, PutCounts, Status};
use vault_proto::peer::exchange::{decode_heads_req, decode_revs_get, encode_revs_get, HeadsItem, HeadsResp, Revs, StateReq};
use vault_proto::peer::{verify, PeerOp, PeerRequest, PeerResponse, PeerStatus};

fn request() -> PeerRequest {
    PeerRequest {
        vault_id: [1; 16],
        sender_device_id: [2; 16],
        receiver_device_id: [3; 16],
        operation: PeerOp::Hello,
        body_sha256: vault_proto::peer::body_hash(&body::empty()),
        t: 1_790_000_000,
        n: [4; 16],
    }
}

#[test]
fn envelopes_round_trip_and_sign_under_their_own_prefixes() {
    let req = request();
    let bytes = req.encode();
    assert_eq!(PeerRequest::decode(&bytes).unwrap(), req);
    let resp = PeerResponse {
        vault_id: [1; 16],
        responder_device_id: [3; 16],
        requester_device_id: [2; 16],
        request_prehash: req.prehash(),
        status: PeerStatus::Ok,
        body_sha256: [9; 32],
        t: 0,
    };
    assert_eq!(PeerResponse::decode(&resp.encode()).unwrap(), resp);
    assert_ne!(req.prehash(), resp.prehash());
    // A provider-request prehash over the same bytes differs (prefixes).
    assert_ne!(req.prehash(), vault_proto::request::prehash_tlv(&bytes));
    let (sk, pk) = dev_keypair_from_scalar([7; 32]);
    let sig = dev_sign_prehash(&sk, &req.prehash());
    assert!(verify(&req.prehash(), &sig, &pk).is_ok());
    assert!(verify(&resp.prehash(), &sig, &pk).is_err(), "a request signature is no response signature");
}

#[test]
fn zero_is_one_byte_and_the_empty_body_is_fixed() {
    let resp = PeerResponse {
        vault_id: [1; 16],
        responder_device_id: [3; 16],
        requester_device_id: [2; 16],
        request_prehash: [0; 32],
        status: PeerStatus::Ok,
        body_sha256: [0; 32],
        t: 0,
    };
    let b = resp.encode();
    // tag 0x06 status: len 1, value 0x00.
    assert!(b.windows(6).any(|w| w == [0x06, 0, 0, 0, 1, 0x00]));
    assert_eq!(body::empty(), vec![0x00, 0, 0, 0, 1, 0xFF]);
}

/// PW-02: every A.1 rule refuses.
#[test]
fn strict_decoding_refuses_every_deviation() {
    let good = request().encode();
    // Trailing byte.
    let mut t = good.clone();
    t.push(0);
    assert_eq!(PeerRequest::decode(&t), Err(ErrorCode::PeerAuthInvalid));
    // Unknown tag (0x09) appended before the end marker.
    let mut u = good[..good.len() - 1].to_vec();
    u.extend_from_slice(&[0x09, 0, 0, 0, 1, 7, 0xFF]);
    assert!(PeerRequest::decode(&u).is_err());
    // A field missing (rebuild without 0x08).
    let missing = EntryBuilder::new()
        .field_uint(1, 1).unwrap().field_bytes(2, &[1; 16]).unwrap().field_bytes(3, &[2; 16]).unwrap()
        .field_bytes(4, &[3; 16]).unwrap().field_uint(5, 1).unwrap().field_bytes(6, &[0; 32]).unwrap()
        .field_uint(7, 1).unwrap().build();
    assert!(PeerRequest::decode(&missing).is_err());
    // An unknown operation code decodes (it is authenticated, then answered
    // with the signed status 4, annex A.1); one beyond u16 does not.
    let mut bytes = request().encode();
    let pos = bytes.windows(6).position(|w| w == [0x05, 0, 0, 0, 1, 0x01]).unwrap();
    bytes[pos + 5] = 0x09;
    assert_eq!(PeerRequest::decode(&bytes).unwrap().operation, PeerOp::Unknown(9));
    let mut wide = request();
    wide.operation = PeerOp::Unknown(0xFFFF);
    assert_eq!(PeerRequest::decode(&wide.encode()).unwrap(), wide);
    // Bodies: an empty value, an unknown tag, a padded integer.
    let empty_value = encode_document(&[EntryBuilder::new().field_bytes(0x01, &[]).unwrap().build()]);
    assert_eq!(decode_heads_req(&empty_value), Err(ErrorCode::FormatInvalid));
    let unknown = encode_document(&[EntryBuilder::new().field_bytes(0x07, &[1]).unwrap().build()]);
    assert!(PutCounts::decode(&unknown).is_err());
    let padded = encode_document(&[EntryBuilder::new().field_bytes(1, &[0, 1]).unwrap().field_bytes(2, &[0]).unwrap().field_bytes(3, &[0]).unwrap().build()]);
    assert!(PutCounts::decode(&padded).is_err());
    // Out-of-order list items.
    let wants = vec![([2u8; 16], vec![]), ([1u8; 16], vec![])];
    assert!(decode_revs_get(&encode_revs_get(&wants)).is_err());
    // An empty have_heads list is absent, never empty.
    let empty_have = encode_document(&[EntryBuilder::new().build(), EntryBuilder::new().field_bytes(1, &[1; 16]).unwrap().field_bytes(2, &[]).unwrap().build()]);
    assert!(decode_revs_get(&empty_have).is_err());
}

#[test]
fn bodies_round_trip() {
    let hello = Hello { registry_seq: 0, registry_head: [5; 32], committed_generation: 0, committed_manifest_hash: [0; 32], heads_digest: heads_digest(&[]) };
    assert_eq!(Hello::decode(&hello.encode()).unwrap(), hello);
    let status = Status { vault_id: [1; 16], registry: vec![0, 0, 0, 0, 0], committed_seq: 3, committed_generation: 7, committed_manifest_hash: [8; 32] };
    assert_eq!(Status::decode(&status.encode()).unwrap(), status);
    let counts = PutCounts { admitted: 0, waiting: 5, refused: 0 };
    assert_eq!(PutCounts::decode(&counts.encode()).unwrap(), counts);
    for req in [StateReq::State { have_generation: 0 }, StateReq::Objects { state_commit: [1; 32], wants: vec![([1; 32], 0), ([2; 32], 4 << 20)] }] {
        assert_eq!(StateReq::decode(&req.encode()).unwrap(), req);
    }
    // Items in covered buckets, bucket by bucket (annex A.3.3).
    let mut items = vec![
        HeadsItem::Heads { record_id: [1; 16], heads: vec![[1; 32], [2; 32]] },
        HeadsItem::Unavailable { record_id: [2; 16], reason: body::TOO_MANY_HEADS },
    ];
    items.sort_by_key(|i| match i { HeadsItem::Heads { record_id, .. } | HeadsItem::Unavailable { record_id, .. } => (bucket(record_id), *record_id) });
    let mut covered = vec![bucket(&[1; 16]), bucket(&[2; 16])];
    covered.sort();
    covered.dedup();
    let heads = HeadsResp { complete: false, buckets: covered, items };
    assert_eq!(HeadsResp::decode(&heads.encode()).unwrap(), heads);
    let wants = vec![([1u8; 16], vec![[3u8; 32]]), ([2u8; 16], vec![])];
    assert_eq!(decode_revs_get(&encode_revs_get(&wants)).unwrap(), wants);
    let get = Revs { complete: Some(true), objects: vec![vec![1, 2, 3]], unavailable: vec![([4; 16], body::TOO_LARGE)] };
    assert_eq!(Revs::decode(&get.encode(), false).unwrap(), get);
    let put = Revs { complete: None, objects: vec![vec![9]], unavailable: vec![] };
    assert_eq!(Revs::decode(&put.encode(), true).unwrap(), put);
    assert!(Revs::decode(&get.encode(), true).is_err(), "a put carries no unavailable entries");
}

#[test]
fn heads_digest_is_order_independent_and_per_bucket() {
    let a = ([1u8; 16], vec![[9u8; 32], [3u8; 32]]);
    let b = ([2u8; 16], vec![[4u8; 32]]);
    let d1 = heads_digest(&[a.clone(), b.clone()]);
    let d2 = heads_digest(&[b.clone(), (a.0, vec![[3u8; 32], [9u8; 32]])]);
    assert_eq!(d1, d2);
    assert_eq!(d1.len(), 256 * 32);
    let empty: [u8; 32] = <sha2::Sha256 as sha2::Digest>::digest(b"").into();
    let untouched = (0..=255u8).find(|x| *x != bucket(&a.0) && *x != bucket(&b.0)).unwrap() as usize;
    assert_eq!(&d1[untouched * 32..untouched * 32 + 32], &empty);
    // Changing one record changes only its bucket.
    let d3 = heads_digest(&[a.clone(), (b.0, vec![[5u8; 32]])]);
    let changed: Vec<usize> = (0..256).filter(|i| d1[i * 32..i * 32 + 32] != d3[i * 32..i * 32 + 32]).collect();
    assert_eq!(changed, vec![bucket(&b.0) as usize]);
}

/// Annex A.5 (review VER-I11): the heads digest formula pinned on a
/// committed input, so a change to it cannot pass unnoticed.
#[test]
fn the_heads_digest_is_pinned() {
    use sha2::Digest;
    let records = vec![([1u8; 16], vec![[9u8; 32], [3u8; 32]]), ([2u8; 16], vec![[4u8; 32]])];
    let d = heads_digest(&records);
    let b1 = bucket(&[1u8; 16]) as usize;
    // One bucket = SHA-256 over (record_id ‖ u16 head_count ‖ heads ascending).
    let mut h = sha2::Sha256::new();
    h.update([1u8; 16]);
    h.update(2u16.to_be_bytes());
    h.update([3u8; 32]);
    h.update([9u8; 32]);
    if bucket(&[2u8; 16]) as usize != b1 {
        assert_eq!(&d[b1 * 32..b1 * 32 + 32], h.finalize().as_slice());
    }
    let whole: [u8; 32] = sha2::Sha256::digest(&d).into();
    assert_eq!(vault_proto::crypto::hex::encode(whole), PINNED);
}

const PINNED: &str = "23dc6d99a506bfa739ac710032f19f0ce00f18d38285d2c53317679091c0bb60";

