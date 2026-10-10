//! The requester's bounds and paging (spec v0.5 §22.8, annex A.3; reviews
//! VER-I1 / VER-O1 of the F.2c fixes): at most 512 records per
//! `peer_revs_get`; a responder that drips one record per page stops at
//! the round-trip bound (`PEER_LIMIT`); a truncated revisions page is
//! re-asked and converges; a heads answer naming a bucket nobody asked for
//! is refused; the streamed-answer guards. Synthetic data only.

mod device_fx;
mod join_fx;
mod sync_fx;
mod vault_fx;

use serde_json::json;
use sync_fx::*;
use vault_fx::*;
use vault_proto::crypto::tlv::EntryReader;
use vault_proto::peer::body::bucket;
use vault_proto::peer::exchange::{HeadsItem, HeadsResp, Revs};

fn body_of(a: &[u8]) -> Vec<u8> {
    EntryReader::parse(a).unwrap().get(3).unwrap().to_vec()
}

/// `n` made-up record ids in bucket `b`, ascending.
fn fake_records(b: u8, n: usize) -> Vec<[u8; 16]> {
    let mut out = Vec::new();
    let mut i = 0u64;
    while out.len() < n {
        let mut id = [0xF0u8; 16];
        id[8..].copy_from_slice(&i.to_be_bytes());
        if bucket(&id) == b {
            out.push(id);
        }
        i += 1;
    }
    out
}

/// The first heads answer gains `n` records the phone lacks (the Mac,
/// asked for them, answers each "not held").
fn with_fake_heads(n: usize) -> impl FnMut(&vault_helper::device::SeDevice, Vec<u8>) -> Vec<u8> {
    let mut done = false;
    move |mac_dev, a| match HeadsResp::decode(&body_of(&a)) {
        Ok(h) if !done && !h.buckets.is_empty() => {
            done = true;
            resign(mac_dev, &a, |_, b| {
                let mut r = h.clone();
                let target = r.buckets[0];
                for id in fake_records(target, n) {
                    r.items.push(HeadsItem::Heads { record_id: id, heads: vec![[0xAB; 32]] });
                }
                r.items.sort_by_key(|it| match it { HeadsItem::Heads { record_id, .. } | HeadsItem::Unavailable { record_id, .. } => (bucket(record_id), *record_id) });
                *b = r.encode();
            })
        }
        _ => a,
    }
}

#[test]
fn at_most_512_records_per_request() {
    let _g = serial();
    let (mac, phone, _) = paired();
    add(&mac, "Bucket Seed");
    fresh(&mac);
    let mac_dev = device(&mac);
    let mut fake = with_fake_heads(600);
    let mut pages = Vec::new();
    let done = sync_with(&mac, &phone, |_, a| {
        let a = fake(&mac_dev, a);
        if let Ok(r) = Revs::decode(&body_of(&a), false) {
            pages.push(r.objects.len() + r.unavailable.len());
        }
        a
    });
    assert_eq!(done["ok"], true, "{done}");
    assert!(pages.len() >= 2 && pages.iter().all(|n| *n <= 512), "pages {pages:?}");
    mac.remove_dir();
    phone.remove_dir();
}

#[test]
fn a_dripping_responder_stops_at_the_round_trip_bound() {
    let _g = serial();
    let (mac, phone, _) = paired();
    add(&mac, "Bucket Seed");
    fresh(&mac);
    let mac_dev = device(&mac);
    let mut fake = with_fake_heads(400);
    let out = sync_with(&mac, &phone, |_, a| {
        vault_helper::peer::verify::reset_rate(); // the Mac's per-minute rate is not under test
        let a = fake(&mac_dev, a);
        match Revs::decode(&body_of(&a), false) {
            Ok(_) => resign(&mac_dev, &a, |_, b| {
                let mut r = Revs::decode(b, false).unwrap();
                r.objects.clear();
                r.unavailable.truncate(1); // one record per page, never complete
                r.complete = Some(false);
                *b = r.encode();
            }),
            Err(_) => a,
        }
    });
    assert_eq!(out["error"], "PEER_LIMIT", "{out}");
    mac.remove_dir();
    phone.remove_dir();
}

#[test]
fn a_truncated_revisions_page_is_re_asked_and_converges() {
    let _g = serial();
    let (mac, phone, _) = paired();
    for i in 0..4 {
        add(&mac, &format!("Paged {i}"));
    }
    fresh(&mac);
    let mac_dev = device(&mac);
    let mut cut = false;
    let done = sync_with(&mac, &phone, |_, a| match Revs::decode(&body_of(&a), false) {
        Ok(r) if !cut && r.objects.len() > 1 => {
            cut = true;
            resign(&mac_dev, &a, |_, b| {
                let first = vault_proto::backup::object::decode(&r.objects[0]).unwrap().record_id;
                let mut t = r.clone();
                t.objects.retain(|o| vault_proto::backup::object::decode(o).unwrap().record_id == first);
                t.unavailable.clear();
                t.complete = Some(false);
                *b = t.encode();
            })
        }
        _ => a,
    });
    assert!(cut, "a multi-record page was truncated");
    assert_eq!(done["ok"], true, "{done}");
    for i in 0..4 {
        assert!(titles(&phone).contains(&format!("Paged {i}")), "item {i} arrived after the re-ask");
    }
    mac.remove_dir();
    phone.remove_dir();
}

#[test]
fn a_bucket_nobody_asked_for_is_refused() {
    let _g = serial();
    let (mac, phone, _) = paired();
    add(&mac, "Bucket Seed");
    fresh(&mac);
    let mac_dev = device(&mac);
    let out = sync_with(&mac, &phone, |_, a| match HeadsResp::decode(&body_of(&a)) {
        Ok(h) => resign(&mac_dev, &a, |_, b| {
            let mut r = h.clone();
            let extra = (0..=255u8).find(|x| !r.buckets.contains(x)).unwrap();
            r.buckets.push(extra);
            r.buckets.sort();
            *b = r.encode();
        }),
        Err(_) => a,
    });
    assert_eq!(out["error"], "PEER_AUTH_INVALID", "{out}");
    mac.remove_dir();
    phone.remove_dir();
}

/// Bytes streamed into an open answer session (as SOURCE Vault does).
fn stream_in(phone: &Fx, session: &str, bytes: &[u8]) {
    let sha = vault_helper::crypto::hex::encode(vault_proto::peer::body_hash(bytes));
    let s = phone.op(json!({"op": "stream_begin", "session": session, "sha256": sha, "size": bytes.len()}));
    let stream = s["stream_id"].as_str().unwrap_or_else(|| panic!("{s}")).to_string();
    let w = phone.op(json!({"op": "stream_write", "session": session, "stream_id": stream, "seq": 0, "offset": 0, "data": vault_proto::b64::encode(bytes)}));
    assert_eq!(w["ok"], true, "{w}");
    assert_eq!(phone.op(json!({"op": "stream_end", "session": session, "stream_id": stream}))["ok"], true);
}

/// The streamed-answer guards (SEC-I1's path): an exchange must be open,
/// the size is capped, only the session opened is consumed, and a lock
/// clears it — each told apart by its own error, with bytes delivered.
#[test]
fn the_streamed_answer_guards_hold() {
    let _g = serial();
    let (mac, phone, _) = paired();
    let junk = vec![0x5Au8; 100];
    let sha = vault_helper::crypto::hex::encode(vault_proto::peer::body_hash(&junk));
    assert_eq!(phone.op(json!({"op": "peer_sync_receive", "sha256": sha, "size": 100}))["error"], "BAD_STATE", "no exchange open");
    assert!(phone.op(json!({"op": "peer_sync_begin"}))["request"].is_string());
    let over = (8u64 << 20) + 4096 + 1;
    assert_eq!(phone.op(json!({"op": "peer_sync_receive", "sha256": sha, "size": over}))["error"], "INVALID_INPUT", "over the cap");
    // Another session id: refused before the bytes reach the exchange.
    let r = phone.op(json!({"op": "peer_sync_receive", "sha256": sha, "size": 100}));
    stream_in(&phone, r["session"].as_str().unwrap(), &junk);
    assert_eq!(phone.op(json!({"op": "peer_sync_step", "session": "11".repeat(16)}))["error"], "TRANSFER_INVALID", "another session");
    // The right session with junk bytes reaches the envelope check.
    let r = phone.op(json!({"op": "peer_sync_receive", "sha256": sha, "size": 100}));
    stream_in(&phone, r["session"].as_str().unwrap(), &junk);
    assert_eq!(phone.op(json!({"op": "peer_sync_step", "session": r["session"]}))["error"], "PEER_AUTH_INVALID", "verified like any answer");
    // A lock clears a delivered answer.
    assert!(phone.op(json!({"op": "peer_sync_begin"}))["request"].is_string());
    let r = phone.op(json!({"op": "peer_sync_receive", "sha256": sha, "size": 100}));
    stream_in(&phone, r["session"].as_str().unwrap(), &junk);
    phone.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    assert_eq!(phone.op(json!({"op": "unlock"}))["ok"], true);
    separate_floors(&phone);
    assert_eq!(phone.op(json!({"op": "peer_sync_step", "session": r["session"]}))["error"], "TRANSFER_INVALID", "the lock cleared it");
    mac.remove_dir();
    phone.remove_dir();
}
