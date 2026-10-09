//! The phone's peer exchange against the Mac's real `peer_serve` (spec
//! v0.5 §22.8; plan `phase-f2c-phone-plan.md`): a paired phone pulls the
//! Mac's new revisions, admitted with the Mac as source; a second sync
//! has nothing to do; local-only revisions wait for the Mac's freshness; a
//! batch too large for one FFI frame streams in (review SEC-I1); a refusal
//! or a lock ends the exchange. Tampering: `phone_sync_tamper.rs`;
//! removal: `phone_sync_removal.rs`. Synthetic data only.

mod device_fx;
mod join_fx;
mod sync_fx;
mod vault_fx;

use serde_json::json;
use sync_fx::*;
use vault_fx::*;
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::storage::sources::{self, Source};
use vault_helper::storage::VaultStore;

#[test]
fn a_phone_pulls_the_macs_new_items_with_the_mac_as_source() {
    let _g = serial();
    let (mac, phone, _) = paired();
    let new_ref = add(&mac, "Added After Pairing");
    fresh(&mac);
    assert!(!titles(&phone).contains("Added After Pairing"));
    let done = sync(&mac, &phone);
    assert_eq!(done["ok"], true, "{done}");
    assert!(done["done"]["admitted"].as_u64().unwrap() >= 1, "{done}");
    assert_eq!(done["done"]["checked"], true, "the signed status was verified");
    assert!(titles(&phone).contains("Added After Pairing"));
    assert!(phone.op(json!({"op": "reveal", "ref": new_ref})).to_string().contains(PASSWORD2));
    // §22.7 provenance: the Mac delivered it — never "provider".
    let store = VaultStore::open(&phone.dir).unwrap();
    let mac_id = device(&mac).device_id();
    let rows = vault_helper::storage::revision_rows::all_rows(&store.conn).unwrap();
    let row = rows.iter().find(|r| r.record_id == new_ref).expect("the new record's revision");
    assert_eq!(sources::of(&store.conn, &row.revision_id).unwrap(), vec![Source::Peer(mac_id)]);
    // Nothing differs any more: the hello digests agree.
    let again = sync(&mac, &phone);
    assert_eq!(again["done"]["admitted"], 0, "{again}");
    mac.remove_dir();
    phone.remove_dir();
}

#[test]
fn local_only_revisions_wait_for_the_macs_freshness() {
    let _g = serial();
    let (mac, phone, _) = paired();
    add(&mac, "Not Yet Confirmed");
    // No verified provider exchange in the last 15 minutes (§22.7).
    let done = sync(&mac, &phone);
    assert_eq!(done["ok"], true, "{done}");
    assert!(!titles(&phone).contains("Not Yet Confirmed"));
    mac.remove_dir();
    phone.remove_dir();
}

/// SEC-I1: an answer over one 64 KiB FFI frame streams into the engine.
#[test]
fn a_large_batch_streams_in() {
    let _g = serial();
    let (mac, phone, _) = paired();
    for i in 0..140 {
        add(&mac, &format!("Bulk Item {i:03}"));
    }
    fresh(&mac);
    let mut largest = 0;
    let done = sync_with(&mac, &phone, |_, a| {
        largest = largest.max(a.len());
        a
    });
    assert_eq!(done["ok"], true, "{done}");
    assert!(largest > 64 * 1024, "the batch exceeded one frame ({largest} bytes)");
    assert!(done["done"]["admitted"].as_u64().unwrap() >= 140, "{done}");
    assert!(titles(&phone).contains("Bulk Item 139"));
    mac.remove_dir();
    phone.remove_dir();
}

/// An unsigned refusal or a lock ends the exchange; nothing continues it.
#[test]
fn a_refusal_or_a_lock_ends_the_exchange() {
    let _g = serial();
    let (mac, phone, _) = paired();
    assert!(phone.op(json!({"op": "peer_sync_begin"}))["request"].is_string());
    assert_eq!(phone.op(json!({"op": "peer_sync_step", "refused": 403}))["error"], "PEER_AUTH_INVALID");
    assert_eq!(phone.op(json!({"op": "peer_sync_step", "response": "AA"}))["error"], "BAD_STATE");
    let begun = phone.op(json!({"op": "peer_sync_begin"}));
    let answer = relay(&mac, &vault_proto::b64::decode(begun["request"].as_str().unwrap()).unwrap()).unwrap();
    phone.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    assert_eq!(phone.op(json!({"op": "unlock"}))["ok"], true);
    separate_floors(&phone);
    assert_eq!(deliver(&phone, &answer)["error"], "BAD_STATE", "the lock ended it");
    assert_eq!(phone.op(json!({"op": "peer_sync_step", "refused": 429}))["error"], "PEER_LIMIT");
    mac.remove_dir();
    phone.remove_dir();
}
