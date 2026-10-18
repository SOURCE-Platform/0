//! What a phone does with what its Mac merely forwards (§22.5, §22.8,
//! §22.12). PS-05 / PS-06 / PS-07 / CX-04: the phone never asks for a
//! forwarded provider state — `peer_state` is not in its exchange and not
//! an op it offers — so a forged, substituted, uncommitted or "NEEDS_USER
//! redo" state, or fork evidence, seen through a peer can neither move its
//! committed tier nor make it COMPROMISED. PS-14: once its committed
//! registry no longer lists its Mac as active, an exchange with that Mac
//! is refused and changes nothing. Synthetic data only.

mod device_fx;
mod join_fx;
mod sync_fx;
mod vault_fx;

use serde_json::json;
use sync_fx::*;
use vault_fx::*;
use vault_helper::registry::device::DeviceIdentity;

/// PS-05, PS-06, PS-07, CX-04 (structural): no code path on the phone
/// requests or applies a forwarded state.
#[test]
fn the_phone_never_asks_for_a_forwarded_state() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let client = std::fs::read_to_string(root.join("vault-engine/src/peer/client/mod.rs")).unwrap();
    assert!(client.contains("PeerOp::Hello") && client.contains("PeerOp::RevsGet"), "the exchange was read");
    assert!(!client.contains("PeerOp::State"), "the phone's exchange never sends peer_state");
    let ffi = std::fs::read_to_string(root.join("vault-ffi/src/engine.rs")).unwrap();
    let ios = &ffi[ffi.find("IOS_OPS").unwrap()..];
    let ios = &ios[..ios.find("];").unwrap()];
    let entries: Vec<&str> = ios.lines().filter(|l| !l.trim_start().starts_with("//")).flat_map(|l| l.split('"').skip(1).step_by(2)).collect();
    assert!(!entries.iter().any(|o| o.contains("peer_state") || o.contains("adopt_forwarded")), "{entries:?}");
}

/// PS-14 / CX-04 in practice: the phone stops trusting its Mac (here: it
/// revoked it itself), then a full exchange attempt with that Mac changes
/// nothing on the phone and never makes it COMPROMISED.
#[test]
fn a_mac_no_longer_active_is_not_believed() {
    let _g = serial();
    let (mac, phone, _item) = paired();
    let before = titles(&phone);
    let mac_id = vault_helper::crypto::hex::encode(device(&mac).device_id());
    phone.push_panel(submitted(MP));
    let r = phone.op(json!({"op": "revoke_device", "device_id": mac_id}));
    assert_eq!(r["ok"], true, "{r}");
    add(&mac, "From A Revoked Mac");
    fresh(&mac);
    let out = sync_with(&mac, &phone, |_, a| a);
    assert_ne!(out["ok"], true, "unable to verify: {out}");
    assert_eq!(titles(&phone), before, "nothing applied, nothing deleted");
    assert_ne!(phone.state(), vault_helper::state::VaultState::Compromised);
    mac.remove_dir();
    phone.remove_dir();
}
