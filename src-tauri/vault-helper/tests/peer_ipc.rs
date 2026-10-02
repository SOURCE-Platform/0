//! `peer_serve` through `dispatch` (wire annex A.2.2; PW-01, PW-08): an
//! inline answer, an unsigned refusal, LOCKED serving, a large response
//! offered as a stream session and read back, and lock aborting it.
//! Synthetic data only.

mod device_fx;
mod peer_fx;
mod vault_fx;

use peer_fx::*;
use serde_json::{json, Value};
use vault_fx::*;
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::storage::store::now_epoch;
use vault_proto::b64;
use vault_proto::peer::body::{empty, Status};
use vault_proto::peer::exchange::{encode_revs_get, Revs};
use vault_proto::peer::{body_hash, PeerOp, PeerRequest, PeerResponse, PeerStatus};

fn frame(w: &W, op: PeerOp, body: Vec<u8>) -> Value {
    w.n.set(w.n.get() + 1);
    let c = ctx(w, true, false, false);
    let req = PeerRequest { vault_id: c.vault_id, sender_device_id: w.id, receiver_device_id: c.me.device_id(), operation: op, body_sha256: body_hash(&body), t: now_epoch(), n: [w.n.get(); 16] };
    let sig = w.phone_dev.sign_prehash(&req.prehash()).unwrap();
    json!({ "op": "peer_serve", "request_tlv": b64::encode(&req.encode()), "signature": b64::encode(&sig), "body": b64::encode(&body) })
}

#[test]
fn peer_serve_answers_inline_and_streams_large_bodies() {
    let _g = serial();
    let w = world("ipc");
    // Inline: a signed status.
    let r = w.fx.op(frame(&w, PeerOp::Status, empty()));
    assert_eq!(r["ok"], true, "{r}");
    let resp = PeerResponse::decode(&b64::decode(r["response_tlv"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(resp.status, PeerStatus::Ok);
    let body = b64::decode(r["body"].as_str().unwrap()).unwrap();
    assert!(Status::decode(&body).is_ok());

    // An unsigned refusal: a replayed request.
    let f = frame(&w, PeerOp::Hello, empty());
    assert_eq!(w.fx.op(f.clone())["ok"], true);
    assert_eq!(w.fx.op(f)["refused"], 403);

    // A large response: enough revisions to pass 24 KiB → stream session.
    let mut ids = Vec::new();
    for _ in 0..60 {
        ids.push(vault_helper::storage::revisions::uuid_bytes(&add_login(&w.fx)).unwrap());
    }
    ids.sort();
    let wants: Vec<([u8; 16], Vec<[u8; 32]>)> = ids.iter().map(|i| (*i, vec![])).collect();
    // Fresh: a verified provider check is simulated by the core flag.
    w.fx.core.lock().unwrap().provider_checked = Some(std::time::Instant::now());
    let r = w.fx.op(frame(&w, PeerOp::RevsGet, encode_revs_get(&wants)));
    assert_eq!(r["ok"], true, "{r}");
    assert!(r.get("body").is_none(), "streamed, not inline");
    let (session, stream, size) = (r["session"].as_str().unwrap(), r["stream"].as_str().unwrap(), r["size"].as_u64().unwrap() as usize);
    let mut got = Vec::new();
    while got.len() < size {
        let chunk = w.fx.op(json!({"op": "stream_read", "session": session, "sha256": stream, "offset": got.len()}));
        assert_eq!(chunk["ok"], true, "{chunk}");
        got.extend(b64::decode(chunk["data"].as_str().unwrap()).unwrap());
    }
    let resp = PeerResponse::decode(&b64::decode(r["response_tlv"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(resp.body_sha256, body_hash(&got), "the streamed body is the signed one");
    assert_eq!(Revs::decode(&got, false).unwrap().objects.len(), 60);

    // Lock aborts the peer session; LOCKED still serves (provider-confirmed
    // only, here: nothing) and signs.
    w.fx.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    let chunk = w.fx.op(json!({"op": "stream_read", "session": session, "sha256": stream, "offset": 0}));
    assert_eq!(chunk["ok"], false);
    let r = w.fx.op(frame(&w, PeerOp::Status, empty()));
    assert_eq!(r["ok"], true, "LOCKED serves: {r}");
    w.fx.remove_dir();
}

fn begin_frame(w: &W, op: PeerOp, body: &[u8], size: u64) -> Value {
    w.n.set(w.n.get() + 1);
    let c = ctx(w, true, false, false);
    let req = PeerRequest { vault_id: c.vault_id, sender_device_id: w.id, receiver_device_id: c.me.device_id(), operation: op, body_sha256: body_hash(body), t: now_epoch(), n: [w.n.get(); 16] };
    let sig = w.phone_dev.sign_prehash(&req.prehash()).unwrap();
    json!({ "op": "peer_serve_begin", "request_tlv": b64::encode(&req.encode()), "signature": b64::encode(&sig), "size": size })
}

fn stream_in(w: &W, session: &str, body: &[u8]) {
    let sha = vault_helper::crypto::hex::encode(body_hash(body));
    let s = w.fx.op(json!({"op": "stream_begin", "session": session, "sha256": sha, "size": body.len()}));
    let stream = s["stream_id"].as_str().unwrap().to_string();
    for (i, chunk) in body.chunks(24 * 1024).enumerate() {
        let r = w.fx.op(json!({"op": "stream_write", "session": session, "stream_id": stream, "seq": i, "offset": i * 24 * 1024, "data": b64::encode(chunk)}));
        assert_eq!(r["ok"], true, "{r}");
    }
    assert_eq!(w.fx.op(json!({"op": "stream_end", "session": session, "stream_id": stream}))["ok"], true);
}

/// PW-03a/b: a large put streams in after the envelope checks; a body
/// over the cap gets a signed status 2 at begin; a revocation landing
/// while the body streams is refused at completion; sessions are single
/// use.
#[test]
fn a_large_put_streams_in_after_the_envelope_checks() {
    use vault_proto::peer::body::PutCounts;
    let _g = serial();
    let w = world("ipc-big");
    let (dir, mut ps, vk) = phone_store(&w);
    let mut rows = Vec::new();
    for i in 0..120 {
        let pt = format!(r#"{{"title":"phone item {i}","username":"p@example.test","password":"synthetic-{i}","urls":[{{"host":"example.test","match":"exact","allow_http":false}}]}}"#);
        let meta = format!(r#"{{"title":"phone item {i}","username":"p@example.test","hosts":["example.test"]}}"#);
        let rid = ps.add_record(&vk, 1, pt.as_bytes(), meta.as_bytes()).unwrap();
        rows.push(row_of(&ps, &rid));
    }
    drop(ps);
    let _ = std::fs::remove_dir_all(dir);
    rows.sort_by_key(|r| vault_helper::storage::revisions::uuid_bytes(&r.record_id).unwrap());
    let body = put_body(&rows);
    assert!(body.len() > 24 * 1024, "needs the streamed path");

    // Over the cap: a signed status 2 at once.
    let r = w.fx.op(begin_frame(&w, PeerOp::RevsPut, &body, (1 << 20) + 1));
    let resp = PeerResponse::decode(&b64::decode(r["response_tlv"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(resp.status, PeerStatus::Limit);

    // The real thing: begin, stream, complete.
    let r = w.fx.op(begin_frame(&w, PeerOp::RevsPut, &body, body.len() as u64));
    let session = r["session"].as_str().unwrap().to_string();
    stream_in(&w, &session, &body);
    let done = w.fx.op(json!({"op": "peer_serve", "session": session}));
    let resp = PeerResponse::decode(&b64::decode(done["response_tlv"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(resp.status, PeerStatus::Ok, "{done}");
    let counts = PutCounts::decode(&b64::decode(done["body"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(counts.admitted, 120);
    assert_eq!(w.fx.op(json!({"op": "peer_serve", "session": session}))["refused"], 403, "single use");

    // A revocation lands while a body streams: refused at completion.
    let r = w.fx.op(begin_frame(&w, PeerOp::RevsPut, &body, body.len() as u64));
    let session = r["session"].as_str().unwrap().to_string();
    stream_in(&w, &session, &body);
    w.fx.push_panel(submitted(MP));
    assert_eq!(w.fx.op(json!({"op": "revoke_device", "device_id": vault_helper::crypto::hex::encode(w.id)}))["ok"], true);
    assert_eq!(w.fx.op(json!({"op": "peer_serve", "session": session}))["refused"], 403);
    w.fx.remove_dir();
}
