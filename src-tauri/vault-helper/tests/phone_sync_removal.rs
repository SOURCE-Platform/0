//! §22.9 on the phone: the Mac removes it; a verified `peer_status` locks
//! the vault, records the removal (pending without a provider), and stops
//! authoring and further exchanges while reads stay; the marker survives a
//! restart and fails closed when torn (review SEC-I3); a marker in the Mac
//! helper's directory changes nothing there (SEC-O1). Synthetic data only.

mod device_fx;
mod join_fx;
mod sync_fx;
mod vault_fx;

use serde_json::json;
use sync_fx::*;
use vault_fx::*;
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::state::VaultState;
use vault_helper::vault::VaultCore;

fn removed_phone() -> (Fx, Fx) {
    let (mac, phone, _) = paired();
    let phone_id = device(&phone).device_id();
    mac.push_panel(submitted(MP)); // the revocation's rotation asks for the master password
    let r = mac.op(json!({"op": "revoke_device", "device_id": vault_helper::crypto::hex::encode(phone_id)}));
    assert_eq!(r["ok"], true, "{r}");
    let out = sync(&mac, &phone);
    assert_eq!(out["removed"]["published"], false, "no provider here: still among the Mac's own entries: {out}");
    (mac, phone)
}

#[test]
fn a_removal_from_the_mac_locks_the_phone() {
    let _g = serial();
    let (mac, phone) = removed_phone();
    assert_eq!(phone.state(), VaultState::Locked);
    let state = phone.core.lock().unwrap().state_answer();
    assert_eq!(state["removed"]["published"], false, "{state}");
    assert_eq!(phone.op(json!({"op": "unlock"}))["ok"], true, "reads stay possible: nothing is deleted");
    separate_floors(&phone);
    assert!(titles(&phone).contains("Fixture Login"), "the vault is still readable");
    assert_eq!(phone.op(json!({"op": "peer_sync_begin"}))["error"], "PEER_NOT_PERMITTED");
    let add = phone.op(json!({"op": "add_item", "kind": "login", "title": "x", "username": "u", "hosts": ["example.test"], "password": "p"}));
    assert_eq!(add["error"], "PEER_NOT_PERMITTED");
    mac.remove_dir();
    phone.remove_dir();
}

#[test]
fn the_lock_survives_a_restart_and_fails_closed() {
    let _g = serial();
    let (mac, phone) = removed_phone();
    // A torn marker (crash mid-write) still locks, and so does a write
    // interrupted before its rename (only the .tmp left).
    std::fs::write(phone.dir.join("removal.json"), b"").unwrap();
    let rebooted = VaultCore::boot_phone(phone.dir.clone());
    assert_eq!(rebooted.state_answer()["removed"]["published"], false, "{}", rebooted.state_answer());
    std::fs::rename(phone.dir.join("removal.json"), phone.dir.join("removal.json.tmp")).unwrap();
    assert!(VaultCore::boot_phone(phone.dir.clone()).state_answer().get("removed").is_some(), "the .tmp alone locks");
    assert_eq!(phone.op(json!({"op": "unlock"}))["ok"], true);
    separate_floors(&phone);
    let add = phone.op(json!({"op": "add_item", "kind": "login", "title": "x", "username": "u", "hosts": ["example.test"], "password": "p"}));
    assert_eq!(add["error"], "PEER_NOT_PERMITTED");
    mac.remove_dir();
    phone.remove_dir();
}

#[test]
fn a_marker_in_the_mac_helpers_directory_changes_nothing() {
    let _g = serial();
    let mac = fx();
    setup_and_unlock(&mac);
    std::fs::write(mac.dir.join("removal.json"), br#"{"published":true,"at":1}"#).unwrap();
    assert!(mac.core.lock().unwrap().state_answer().get("removed").is_none());
    add_login(&mac);
    mac.remove_dir();
}
