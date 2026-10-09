//! PA-05 and the requester's other checks (spec v0.5 §22.8, §22.9): an
//! answer replayed from another exchange, a well-formed body swapped by
//! main, an answer signed or addressed by another device, a `peer_status`
//! that rewrites committed entries or carries an unverifiable one, a wrong
//! vault or `committed_seq`, an unrequested record — each is "unable to
//! verify", nothing applied. Paging a cap left out converges. A lying Mac
//! helper is simulated by re-signing with the Mac's own key. Synthetic.

mod device_fx;
mod join_fx;
mod sync_fx;
mod vault_fx;

use serde_json::Value;
use sync_fx::*;
use vault_fx::*;
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::registry::file;
use vault_proto::crypto::tlv::EntryReader;
use vault_proto::peer::body::{Hello, Status};
use vault_proto::peer::exchange::{HeadsResp, Revs};

fn body_of(a: &[u8]) -> Vec<u8> {
    EntryReader::parse(a).unwrap().get(3).unwrap().to_vec()
}

/// Re-sign every `peer_revs_get` answer after `f` edits it.
fn edit_revs<'a>(dev: &'a vault_helper::device::SeDevice, f: impl Fn(&mut Revs) + 'a) -> impl FnMut(usize, Vec<u8>) -> Vec<u8> + 'a {
    move |_i, a| match Revs::decode(&body_of(&a), false) {
        Ok(_) => resign(dev, &a, |_, b| {
            let mut r = Revs::decode(b, false).unwrap();
            f(&mut r);
            *b = r.encode();
        }),
        Err(_) => a,
    }
}

fn refused(out: &Value, label: &str) {
    assert_eq!(out["error"], "PEER_AUTH_INVALID", "{label}: {out}");
}

#[test]
fn answers_from_another_exchange_or_with_a_swapped_body_are_refused() {
    let _g = serial();
    let (mac, phone, _) = paired();
    add(&mac, "Should Not Arrive");
    fresh(&mac);
    // A status answer from an earlier exchange, replayed into a new one:
    // same operation, valid signature, another request_prehash.
    let mut old = None;
    let out = sync_with(&mac, &phone, |i, a| {
        if i == 0 {
            old = Some(a.clone());
        }
        a
    });
    assert_eq!(out["ok"], true, "{out}");
    let later = add(&mac, "Later Item");
    fresh(&mac);
    refused(&sync_with(&mac, &phone, |i, a| if i == 0 { old.clone().unwrap() } else { a }), "replayed status");
    // A well-formed hello body of the right type, swapped in by main.
    let swap = |a: Vec<u8>| {
        let e = EntryReader::parse(&a).unwrap();
        let mut h = Hello::decode(e.get(3).unwrap()).unwrap();
        h.registry_seq += 1;
        carriage(e.get(1).unwrap(), e.get(2).unwrap(), &h.encode())
    };
    refused(&sync_with(&mac, &phone, |i, a| if i == 1 { swap(a) } else { a }), "swapped hello body");
    assert!(!titles(&phone).contains("Later Item"), "nothing applied: {later}");
    mac.remove_dir();
    phone.remove_dir();
}

#[test]
fn answers_signed_or_addressed_by_another_device_are_refused() {
    let _g = serial();
    let (mac, phone, _) = paired();
    let other = device(&phone); // an active device that is not the Mac
    let other_id = other.device_id();
    refused(&sync_with(&mac, &phone, |i, a| if i == 0 { resign(&other, &a, |r, _| r.responder_device_id = other_id) } else { a }), "another responder");
    refused(&sync_with(&mac, &phone, |i, a| if i == 0 { resign(&other, &a, |_, _| {}) } else { a }), "another signer");
    let mac_dev = device(&mac);
    refused(&sync_with(&mac, &phone, |i, a| if i == 0 { resign(&mac_dev, &a, |r, _| r.requester_device_id = [0xEE; 16]) } else { a }), "addressed elsewhere");
    refused(&sync_with(&mac, &phone, |i, a| if i == 0 { resign(&mac_dev, &a, |r, _| r.vault_id = [0xEE; 16]) } else { a }), "another vault");
    mac.remove_dir();
    phone.remove_dir();
}

/// §22.9: a signed status whose chain rewrites a committed entry, carries
/// an entry that does not verify, names another vault, or a
/// `committed_seq` beyond its chain — "unable to verify", never a lock.
#[test]
fn a_status_that_does_not_extend_the_committed_chain_is_refused() {
    let _g = serial();
    let (mac, phone, _) = paired();
    let mac_dev = device(&mac);
    let edit_status = |f: &dyn Fn(&mut Status)| {
        let f = |a: Vec<u8>| resign(&mac_dev, &a, |_, b| {
            let mut s = Status::decode(b).unwrap();
            f(&mut s);
            *b = s.encode();
        });
        sync_with(&mac, &phone, |i, a| if i == 0 { f(a) } else { a })
    };
    refused(&edit_status(&|s| {
        let mut entries = file::decode(&s.registry).unwrap();
        entries[1].device_name = Some("Forged Name".into());
        s.registry = file::encode(&entries).unwrap();
    }), "rewritten committed entry");
    refused(&edit_status(&|s| s.vault_id = [0xEE; 16]), "another vault");
    refused(&edit_status(&|s| s.committed_seq = 99), "committed_seq beyond the chain");
    // The Mac revokes the phone; the revocation entry is then mangled.
    let phone_id = device(&phone).device_id();
    mac.push_panel(submitted(MP));
    assert_eq!(mac.op(serde_json::json!({"op": "revoke_device", "device_id": vault_helper::crypto::hex::encode(phone_id)}))["ok"], true);
    refused(&edit_status(&|s| {
        let mut entries = file::decode(&s.registry).unwrap();
        let last = entries.last_mut().unwrap();
        last.signature = last.signature.map(|mut sig| { sig[5] ^= 1; sig });
        s.registry = file::encode(&entries).unwrap();
    }), "unverifiable revocation");
    assert!(vault_helper::peer::client::removal::active(&phone.dir).is_none(), "never a lock on unverifiable evidence");
    mac.remove_dir();
    phone.remove_dir();
}

#[test]
fn an_unrequested_record_is_refused_and_a_stalled_page_ends_limited() {
    let _g = serial();
    let (mac, phone, _) = paired();
    add(&mac, "Paged Item");
    fresh(&mac);
    let mac_dev = device(&mac);
    refused(&sync_with(&mac, &phone, edit_revs(&mac_dev, |r| r.unavailable.push(([0xEE; 16], 2)))), "unrequested record");
    let stalled = sync_with(&mac, &phone, edit_revs(&mac_dev, |r| {
        r.complete = Some(false);
        r.objects.clear();
        r.unavailable.clear();
    }));
    assert_eq!(stalled["done"]["limited"], true, "no progress: left to the provider: {stalled}");
    assert!(!titles(&phone).contains("Paged Item"));
    mac.remove_dir();
    phone.remove_dir();
}

/// A heads answer covering only some buckets (`complete = 0`): the phone
/// re-asks the rest and converges.
#[test]
fn heads_paging_converges() {
    let _g = serial();
    let (mac, phone, _) = paired();
    for i in 0..8 {
        add(&mac, &format!("Spread Item {i}"));
    }
    fresh(&mac);
    let mac_dev = device(&mac);
    let mut cut = false;
    let done = sync_with(&mac, &phone, |_, a| match HeadsResp::decode(&body_of(&a)) {
        Ok(h) if !cut && h.buckets.len() > 1 => {
            cut = true;
            resign(&mac_dev, &a, |_, b| {
                let keep = h.buckets[0];
                let mut r = h.clone();
                r.complete = false;
                r.buckets = vec![keep];
                r.items.retain(|it| match it {
                    vault_proto::peer::exchange::HeadsItem::Heads { record_id, .. } | vault_proto::peer::exchange::HeadsItem::Unavailable { record_id, .. } => vault_proto::peer::body::bucket(record_id) == keep,
                });
                *b = r.encode();
            })
        }
        _ => a,
    });
    assert!(cut, "the first heads answer covered several buckets");
    assert_eq!(done["ok"], true, "{done}");
    for i in 0..8 {
        assert!(titles(&phone).contains(&format!("Spread Item {i}")), "item {i} arrived after the re-ask");
    }
    mac.remove_dir();
    phone.remove_dir();
}
