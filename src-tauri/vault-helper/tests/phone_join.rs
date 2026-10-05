//! A phone joins a vault its Mac authorizes (spec §5, v0.5 §22.10): the
//! engine's phone-side ops against the Mac's real enrollment ops, end to
//! end — QR, hello, the code shown by the Mac's Source Vault window, the
//! bundle streamed in, the §22.10 checks, ACK — then the phone unlocks
//! with its own envelope and holds the Mac's records. Every tampered
//! bundle is refused and leaves nothing behind (MT-01). Synthetic data
//! only.

mod device_fx;
mod join_fx;
mod vault_fx;

use join_fx::*;
use serde_json::json;
use vault_fx::*;
use vault_helper::crypto::hex;
use vault_helper::state::VaultState;
use vault_helper::vault::join_ops;

#[test]
fn a_phone_joins_unlocks_and_holds_the_macs_records() {
    let _g = serial();
    let mac = fx();
    setup_and_unlock(&mac);
    let item = add_login(&mac);
    let phone = fx();
    let (bundle, codes) = pair_up_to_bundle(&mac, &phone, |_| {});
    assert_eq!(codes, "match", "the phone and the Source Vault window show the same code");
    let done = complete(&phone, &serde_json::to_vec(&bundle).unwrap());
    assert_eq!(done["ok"], true, "{done}");
    assert_eq!(phone.state(), VaultState::Locked, "materialized, the key not kept");
    // The routes answer only the device that sent the hello (SEC-I3).
    assert_eq!(mac.op(json!({"op": "enroll_proof", "proof": "00".repeat(32)}))["ok"], false);
    assert_eq!(mac.op(json!({"op": "enroll_proof", "proof": done["proof"]}))["ok"], true);
    let acked = mac.op(json!({"op": "enroll_ack", "signature": done["ack"]["signature"]}));
    assert_eq!(acked["ok"], true, "the Mac accepts the phone's ACK: {acked}");
    assert_eq!(phone.op(json!({"op": "join_finish"}))["ok"], true);
    assert_eq!(phone.op(json!({"op": "join_abort"}))["ok"], false, "a finished join cannot be undone this way");
    let ep = join_ops::stored_peer_endpoint().unwrap().expect("peer endpoint in the Keychain");
    assert_eq!(ep["port"], 50124);

    let unlocked = phone.op(json!({"op": "unlock"}));
    assert_eq!(unlocked["ok"], true, "{unlocked}");
    assert!(phone.op(json!({"op": "list_items"}))["items"].to_string().contains(&item));
    assert!(phone.op(json!({"op": "reveal", "ref": item})).to_string().contains(PASSWORD));
    mac.remove_dir();
    phone.remove_dir();
}

/// SEC-B3 (owner decision 2026-10-03): a QR naming another Mac key gives
/// the phone a different code from the Source Vault window's — and the
/// phone refuses the bundle even if the user went on.
#[test]
fn a_qr_with_another_mac_key_shows_a_different_code_and_is_refused() {
    let _g = serial();
    let mac = fx();
    setup_and_unlock(&mac);
    let phone = fx();
    let (bundle, codes) = pair_up_to_bundle(&mac, &phone, |q| q["mac_key"] = json!("cd".repeat(32)));
    assert_eq!(codes, "differ");
    assert_eq!(complete(&phone, &serde_json::to_vec(&bundle).unwrap())["ok"], false);
    nothing_left(&phone);
    mac.remove_dir();
    phone.remove_dir();
}

#[test]
fn a_registry_without_this_phones_entry_is_refused() {
    refused("registry", |b| {
        let entries = vault_helper::registry::file::decode(&hex::decode(b["registry"].as_str().unwrap()).unwrap()).unwrap();
        b["registry"] = json!(hex::encode(vault_helper::registry::file::encode(&entries[..entries.len() - 1]).unwrap()));
    });
}

#[test]
fn another_registry_head_is_refused() {
    refused("head", |b| b["registry_head"] = json!("11".repeat(32)));
}

#[test]
fn a_manifest_with_a_broken_signature_is_refused() {
    refused("manifest", |b| {
        let mut m = hex::decode(b["manifest"].as_str().unwrap()).unwrap();
        let n = m.len() - 3;
        m[n] ^= 0x01;
        b["manifest"] = json!(hex::encode(m));
    });
}

#[test]
fn a_checkpoint_not_under_the_vault_key_is_refused() {
    refused("checkpoint", |b| {
        let mut c = hex::decode(b["checkpoint"].as_str().unwrap()).unwrap();
        let n = c.len() - 3;
        c[n] ^= 0x01;
        b["checkpoint"] = json!(hex::encode(c));
    });
}

/// SEC-B1: an envelope beside the signed index — whatever it seals — is
/// never the one opened.
#[test]
fn an_envelope_other_than_the_indexed_one_is_refused() {
    refused("envelope", |b| {
        let enc = b["envelope"]["enc"].as_str().unwrap().to_string();
        b["envelope"]["enc"] = json!(format!("{}{}", &enc[..enc.len() - 2], if enc.ends_with("00") { "01" } else { "00" }));
    });
}

#[test]
fn an_object_changed_under_its_hash_is_refused() {
    refused("object", |b| {
        let objects = b["objects"].as_array_mut().unwrap();
        let data = objects[0][1].as_str().unwrap().to_string();
        let flipped = if data.starts_with('0') { format!("1{}", &data[1..]) } else { format!("0{}", &data[1..]) };
        objects[0][1] = json!(flipped);
    });
}

#[test]
fn a_bad_peer_endpoint_is_refused() {
    refused("endpoint", |b| b["peer_endpoint"]["token"] = json!("not-a-token"));
}

#[test]
fn a_hello_reply_from_another_mac_is_refused() {
    let _g = serial();
    let mac = fx();
    setup_and_unlock(&mac);
    let phone = fx();
    let begun = device_fx::begin(&mac);
    let started = phone.op(json!({"op": "join_begin", "qr": qr(&begun), "name": "Synthetic iPhone"}));
    let mut hello = started["hello"].clone();
    hello["op"] = json!("enroll_hello");
    let mut reply = mac.op(hello)["reply"].clone();
    reply["mac_device_id"] = json!("ee".repeat(16));
    assert_eq!(phone.op(json!({"op": "join_hello", "reply": reply}))["ok"], false);
    assert_eq!(phone.op(json!({"op": "join_abort"}))["ok"], true);
    nothing_left(&phone);
    mac.remove_dir();
    phone.remove_dir();
}

/// VER-I1: an attempt interrupted after the vault was written (the app
/// killed before the ACK) is removed whole at the next start.
#[test]
fn an_interrupted_join_is_removed_at_the_next_start() {
    let _g = serial();
    let mac = fx();
    setup_and_unlock(&mac);
    let phone = fx();
    let (bundle, _) = pair_up_to_bundle(&mac, &phone, |_| {});
    assert_eq!(complete(&phone, &serde_json::to_vec(&bundle).unwrap())["ok"], true);
    assert!(phone.dir.join(vault_helper::VAULT_HEADER_NAME).exists());
    let rebooted = vault_helper::vault::VaultCore::boot_phone(phone.dir.clone());
    assert_eq!(rebooted.state, VaultState::Uninitialized);
    assert!(!phone.dir.join(vault_helper::VAULT_HEADER_NAME).exists());
    assert!(join_ops::stored_peer_endpoint().unwrap().is_none());
    mac.remove_dir();
    phone.remove_dir();
}
