//! XV-PEER vectors (spec v0.5 §22.8, wire annex revision 3 A.5 with the
//! 2026-10-09 errata): one signed request and response per operation, the
//! carriage entries, the empty body, the heads digest, a canonical
//! revision batch, and one invalid case per A.1 rule. Checked by the Rust
//! engine (`tests/xv_peer.rs`) and by SOURCE Vault's CryptoKit-only target.
//! Fixed synthetic inputs only.

use serde_json::{json, Value};
use vault_proto::crypto::recovery_auth::RecoveryClass;
use vault_proto::crypto::tlv::EntryBuilder;
use vault_proto::peer::body::{self, heads_digest, Hello, PutCounts, Status, NOT_HELD, TOO_LARGE, TOO_MANY_HEADS};
use vault_proto::peer::exchange::{encode_heads_req, encode_revs_get, encode_state, HeadsItem, HeadsResp, ObjectItem, Objects, Revs, StateReq};
use vault_proto::peer::{body_hash, PeerOp, PeerRequest, PeerResponse, PeerStatus};
use vault_proto::state::{recovery_auth_digest, state_commit, RecoveryAuthEntry};

use super::ecdsa::{dev_keypair_from_scalar, dev_sign_prehash};
use super::hex;
use super::vectors_peer_data::{self as data, rev, sha};

const VAULT_ID: [u8; 16] = [0xA0; 16];
const PHONE_ID: [u8; 16] = [0xB1; 16];
const MAC_ID: [u8; 16] = [0xC2; 16];
const PHONE_SCALAR: [u8; 32] = [0x33; 32];
const MAC_SCALAR: [u8; 32] = [0x11; 32];
const T0: u64 = 1_790_000_000;

fn signed(prehash: &[u8; 32], scalar: [u8; 32]) -> Vec<u8> {
    let (sk, _) = dev_keypair_from_scalar(scalar);
    dev_sign_prehash(&sk, prehash).to_vec()
}

/// Annex A.2.1: `{0x01 tlv, 0x02 signature, 0x03 body}`.
fn carriage(tlv: &[u8], sig: &[u8], body: &[u8]) -> Vec<u8> {
    EntryBuilder::new()
        .field_bytes(0x01, tlv)
        .and_then(|b| b.field_bytes(0x02, sig))
        .and_then(|b| b.field_bytes(0x03, body))
        .expect("ascending tags")
        .build()
}

fn part(tlv: &[u8], prehash: [u8; 32], sig: &[u8], body: &[u8]) -> Value {
    json!({
        "tlv": hex::encode(tlv), "prehash": hex::encode(prehash), "signature": hex::encode(sig),
        "body": hex::encode(body), "body_sha256": sha(body), "carriage": hex::encode(carriage(tlv, sig, body)),
    })
}

/// One signed exchange: the phone asks, the Mac answers.
fn exchange(name: &str, k: u8, op: PeerOp, req_body: Vec<u8>, status: PeerStatus, resp_body: Vec<u8>) -> Value {
    let req = PeerRequest {
        vault_id: VAULT_ID,
        sender_device_id: PHONE_ID,
        receiver_device_id: MAC_ID,
        operation: op,
        body_sha256: body_hash(&req_body),
        t: T0 + u64::from(k),
        n: [k; 16],
    };
    let resp = PeerResponse {
        vault_id: VAULT_ID,
        responder_device_id: MAC_ID,
        requester_device_id: PHONE_ID,
        request_prehash: req.prehash(),
        status,
        body_sha256: body_hash(&resp_body),
        t: T0 + u64::from(k) + 1,
    };
    json!({
        "name": name,
        "operation": op.code(),
        "status": status.code(),
        "request": part(&req.encode(), req.prehash(), &signed(&req.prehash(), PHONE_SCALAR), &req_body),
        "response": part(&resp.encode(), resp.prehash(), &signed(&resp.prehash(), MAC_SCALAR), &resp_body),
    })
}

/// A state body through the shipped A.3.2 encoder, with an mp and an rk
/// recovery-auth entry; opaque synthetic manifest and checkpoint bytes
/// (their own families cover their formats).
fn state() -> (Vec<u8>, [u8; 32], Vec<RecoveryAuthEntry>) {
    let (manifest, checkpoint) = (b"synthetic-manifest".to_vec(), b"synthetic-checkpoint".to_vec());
    let auth = vec![
        RecoveryAuthEntry { class: RecoveryClass::Mp, public: dev_keypair_from_scalar([0x55; 32]).1, salt: [0x5A; 16] },
        RecoveryAuthEntry { class: RecoveryClass::Rk, public: dev_keypair_from_scalar([0x66; 32]).1, salt: [0x6B; 16] },
    ];
    let digest = recovery_auth_digest(&auth).expect("one per class");
    let commit = state_commit(&VAULT_ID, 4, &body_hash(&manifest), &body_hash(&checkpoint), &digest);
    (crate::sync::remote::encode_fields(4, 1, &commit, &manifest, &checkpoint, &auth), commit, auth)
}

pub fn xv_peer() -> Value {
    let (r, empty_bucket) = data::records();
    let same_bucket = body::bucket(&r[0]);
    let mut buckets = vec![same_bucket, empty_bucket];
    buckets.sort();
    let g = data::graph(&r);
    let mut batch = data::objects(&g.canonical);
    batch.extend(data::objects(&g.second));
    let peer_heads = data::heads(&r);
    let digest = heads_digest(&peer_heads);
    let (state, commit, auth) = state();
    let hello = Hello { registry_seq: 3, registry_head: [0x21; 32], committed_generation: 4, committed_manifest_hash: [0x22; 32], heads_digest: digest.clone() }.encode();
    let (h1, h2, h3) = ([0x71u8; 32], [0x72u8; 32], [0x73u8; 32]);
    // Three asked; the third did not fit, so complete = 0 (A.3).
    let objects_req = StateReq::Objects { state_commit: commit, wants: vec![(h1, 0), (h2, 4 << 20), (h3, 0)] }.encode();
    let objects_resp = Objects {
        complete: false,
        items: vec![
            ObjectItem::Unavailable { sha256: h1, reason: NOT_HELD },
            ObjectItem::Chunk { sha256: h2, offset: 4 << 20, total_len: (4 << 20) + 5, bytes: b"tail!".to_vec() },
        ],
    }
    .encode();
    let heads_resp = HeadsResp {
        complete: true,
        buckets: buckets.clone(),
        items: vec![
            HeadsItem::Unavailable { record_id: r[0], reason: TOO_MANY_HEADS },
            HeadsItem::Heads { record_id: r[1], heads: peer_heads[1].1.clone() },
            HeadsItem::Heads { record_id: r[2], heads: peer_heads[2].1.clone() },
        ],
    }
    .encode();
    let wants = vec![(r[0], vec![]), (r[1], vec![]), (r[2], vec![rev(0x5F)])];
    let revs_resp = Revs { complete: Some(true), objects: batch, unavailable: vec![(r[0], TOO_LARGE)] }.encode();
    let put = Revs { complete: None, objects: data::objects(&g.second), unavailable: vec![] }.encode();
    let status = Status { vault_id: VAULT_ID, registry: b"synthetic-registry-file".to_vec(), committed_seq: 3, committed_generation: 4, committed_manifest_hash: [0x22; 32] }.encode();
    let counts = |a, w, x| PutCounts { admitted: a, waiting: w, refused: x }.encode();
    let empty = body::empty;
    let list = vec![
        exchange("peer_hello (empty request body, erratum A.3.1)", 1, PeerOp::Hello, empty(), PeerStatus::Ok, hello),
        exchange("peer_state (state mode)", 2, PeerOp::State, StateReq::State { have_generation: 3 }.encode(), PeerStatus::Ok, encode_state(&state)),
        exchange("peer_state (objects mode, byte range, complete = 0)", 3, PeerOp::State, objects_req, PeerStatus::Ok, objects_resp),
        exchange("peer_heads (three records in one bucket, one reason 4, an empty bucket)", 4, PeerOp::Heads, encode_heads_req(&buckets), PeerStatus::Ok, heads_resp),
        exchange("peer_revs_get (canonical order, unavailable after the objects)", 5, PeerOp::RevsGet, encode_revs_get(&wants), PeerStatus::Ok, revs_resp),
        exchange("peer_revs_put", 6, PeerOp::RevsPut, put.clone(), PeerStatus::Ok, counts(1, 0, 0)),
        exchange("peer_revs_put to a LOCKED Mac", 7, PeerOp::RevsPut, put, PeerStatus::Ok, counts(0, 1, 0)),
        exchange("peer_status", 8, PeerOp::Status, empty(), PeerStatus::Ok, status),
        exchange("unknown operation → status 4", 9, PeerOp::Unknown(9), empty(), PeerStatus::FormatInvalid, empty()),
    ];
    let good_request = hex::decode(list[0]["request"]["tlv"].as_str().expect("tlv")).expect("hex");
    let (_, phone_pub) = dev_keypair_from_scalar(PHONE_SCALAR);
    let (_, mac_pub) = dev_keypair_from_scalar(MAC_SCALAR);
    json!({
        "family": "XV-PEER",
        "annex": "phase-f2-peer-wire-annex.md revision 3, A.5, with the 2026-10-09 errata",
        "keys": {
            "phone_scalar_dev_only": hex::encode(PHONE_SCALAR), "phone_pub": hex::encode(phone_pub),
            "mac_scalar_dev_only": hex::encode(MAC_SCALAR), "mac_pub": hex::encode(mac_pub),
            "vault_id": hex::encode(VAULT_ID), "phone_device_id": hex::encode(PHONE_ID), "mac_device_id": hex::encode(MAC_ID),
        },
        "empty_body": { "hex": hex::encode(body::empty()), "sha256": sha(&body::empty()) },
        "zero_integer": hex::encode(body::uint(0)),
        "exchanges": list,
        "heads_digest": {
            "records": peer_heads.iter().map(|(id, hs)| json!({ "record_id": hex::encode(id), "heads": hs.iter().map(hex::encode).collect::<Vec<_>>() })).collect::<Vec<_>>(),
            "bucket": same_bucket, "empty_bucket": empty_bucket,
            "empty_bucket_digest": sha(b""),
            "digest": hex::encode(&digest), "digest_sha256": sha(&digest),
        },
        "state": {
            "vault_id": hex::encode(VAULT_ID), "state_commit": hex::encode(commit),
            "recovery_auth_digest": hex::encode(recovery_auth_digest(&auth).expect("one per class")),
        },
        "revs_batch": {
            "records": r.iter().map(hex::encode).collect::<Vec<_>>(),
            "canonical_record_1": data::graph_json(&g.canonical),
            "fifo_record_1": data::graph_json(&g.fifo),
            "depth_first_record_1": data::graph_json(&g.depth_first),
        },
        "invalid": data::invalid_cases(&good_request, &r, &g),
    })
}
