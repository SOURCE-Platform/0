//! Phase E §5: Mac ↔ device enrollment end to end, and the failures the
//! protocol has to refuse.
//!
//! The "phone" here is a second, independent Secure Enclave identity on
//! this Mac: it generates its own SE signing and agreement keys, opens
//! the HPKE envelope inside its own Enclave, and signs the ACK with a
//! key the enrolling side never sees. That exercises exactly the
//! production code path on both sides. Synthetic vaults and credentials
//! only.

mod device_fx;
mod vault_fx;

use serde_json::json;
use sha2::{Digest, Sha256};
use device_fx::*;
use vault_fx::*;
use vault_helper::backup::manifest::SignedManifest;
use vault_helper::crypto::hex;
use vault_helper::device::envelope;
use vault_helper::enroll::transcript;
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::registry::log;

/// EN-01: the whole §5.1 sequence, with the phone opening its envelope
/// in its own Enclave and signing the ACK with its own SE key.
#[test]
fn enrollment_end_to_end() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let phone = Phone::new("e2e");

    let begun = begin(&fx);
    let secret = begun["secret"].as_str().unwrap().to_string();
    assert_eq!(secret.len(), 26, "16-byte secret in the repo's base32");
    let vault_id = hex::decode_array::<16>(begun["vault_id"].as_str().unwrap()).unwrap();

    let nonce_n = [0x42u8; 16];
    let hello = fx.op(phone.hello(&secret, nonce_n));
    assert_eq!(hello["ok"], true, "enroll_hello: {hello}");
    let reply = &hello["reply"];
    let mac_sas = hello["sas"].as_str().unwrap().to_string();
    assert_eq!(mac_sas.len(), 8);
    // Both sides derive the same 8 characters from the same transcript.
    assert_eq!(mac_sas, phone_sas(&phone, &secret, nonce_n, reply));
    let new_id = hex::decode_array::<16>(reply["new_device_id"].as_str().unwrap()).unwrap();

    // The user compares, confirms, and passes the Mac's presence check.
    let confirmed = fx.op(json!({"op": "enroll_confirm"}));
    assert_eq!(confirmed["ok"], true, "enroll_confirm: {confirmed}");
    let bundle = &confirmed["bundle"];
    let head = hex::decode_array::<32>(bundle["registry_head"].as_str().unwrap()).unwrap();

    // Nothing is in the registry yet: the entry lands only on ACK.
    let before = log::read_entries(&fx.dir).unwrap();
    assert_eq!(before.len(), 1, "only genesis until the ACK arrives");

    // The phone opens its envelope inside its Enclave.
    let env: envelope::DeviceEnvelopeFile =
        serde_json::from_value(bundle["envelope"].clone()).unwrap();
    let payload = envelope::open_envelope(phone.dev.key_tag(), &vault_id, &env).expect("envelope");
    assert_eq!(payload.vk_generation, 1);
    assert_eq!(payload.device_backup_cred.expose().len(), 32);

    // ...verifies the manifest it was sent, signed by the Mac.
    let manifest =
        SignedManifest::decode(&hex::decode(bundle["manifest"].as_str().unwrap()).unwrap()).unwrap();
    let mac_sign_pub = {
        let entries = log::read_entries(&fx.dir).unwrap();
        entries[0].sign_pub.unwrap()
    };
    manifest.verify(&mac_sign_pub).expect("manifest signature");
    assert!(!bundle["objects"].as_array().unwrap().is_empty());

    // ...and proves its signing key exists by ACKing the head.
    let digest = transcript::ack_digest(&head, &hex::decode_array::<16>(reply["mac_device_id"].as_str().unwrap()).unwrap());
    let sig = phone.dev.sign_prehash(&digest).unwrap();
    let acked = fx.op(json!({"op": "enroll_ack", "signature": hex::encode(sig)}));
    assert_eq!(acked["ok"], true, "enroll_ack: {acked}");

    // Now the registry has the enroll entry, and the heads moved together.
    let after = log::read_entries(&fx.dir).unwrap();
    assert_eq!(after.len(), 2);
    assert_eq!(after[1].device_id, new_id);
    assert_eq!(after[1].sign_pub.unwrap(), phone.dev.sign_pub());
    let listed = fx.op(json!({"op": "list_devices"}));
    assert_eq!(listed["devices"].as_array().unwrap().len(), 2);
    assert_eq!(listed["registry_head"], hex::encode(head));
    // The Mac keeps a copy of the device's envelope so it can re-seal it
    // on a later VK rotation (§11.4).
    assert!(envelope::read_envelope(&fx.dir, &new_id).is_ok());
}

/// EN-02: a wrong secret is refused, and five of them end the session
/// (§5.2 single-use secret, §5.3 "close after 5 tries").
#[test]
fn wrong_secret_five_times_tears_the_session_down() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let phone = Phone::new("secret");
    let begun = begin(&fx);
    let real = begun["secret"].as_str().unwrap().to_string();
    let wrong = transcript::encode_secret(&[0u8; 16]);
    assert_ne!(wrong, real);
    for _ in 0..4 {
        let resp = fx.op(phone.hello(&wrong, [1u8; 16]));
        assert_eq!(err_code(&resp), "WRONG_CREDENTIAL", "{resp}");
    }
    let resp = fx.op(phone.hello(&wrong, [1u8; 16]));
    assert_eq!(err_code(&resp), "WRONG_CREDENTIAL");
    // Session gone: even the right secret no longer works.
    let resp = fx.op(phone.hello(&real, [1u8; 16]));
    assert_eq!(err_code(&resp), "BAD_STATE", "{resp}");
}

/// EN-03: the secret is single-use — a replayed hello on a session that
/// has moved on is refused.
#[test]
fn hello_cannot_be_replayed() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let phone = Phone::new("replay");
    let begun = begin(&fx);
    let secret = begun["secret"].as_str().unwrap().to_string();
    assert_eq!(fx.op(phone.hello(&secret, [2u8; 16]))["ok"], true);
    let again = fx.op(phone.hello(&secret, [2u8; 16]));
    assert_eq!(err_code(&again), "BAD_STATE", "{again}");
}

/// EN-04: a forged ACK (signed by a different key) never enrolls anyone,
/// and the session is destroyed rather than retried.
#[test]
fn forged_ack_is_rejected_and_writes_nothing() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let phone = Phone::new("ack-good");
    let attacker = Phone::new("ack-bad");
    let begun = begin(&fx);
    let secret = begun["secret"].as_str().unwrap().to_string();
    let hello = fx.op(phone.hello(&secret, [3u8; 16]));
    assert_eq!(hello["ok"], true);
    let mac_id =
        hex::decode_array::<16>(hello["reply"]["mac_device_id"].as_str().unwrap()).unwrap();
    let confirmed = fx.op(json!({"op": "enroll_confirm"}));
    let head =
        hex::decode_array::<32>(confirmed["bundle"]["registry_head"].as_str().unwrap()).unwrap();
    let digest = transcript::ack_digest(&head, &mac_id);
    let forged = attacker.dev.sign_prehash(&digest).unwrap();
    let resp = fx.op(json!({"op": "enroll_ack", "signature": hex::encode(forged)}));
    assert_eq!(err_code(&resp), "SIGNATURE_INVALID", "{resp}");
    assert_eq!(log::read_entries(&fx.dir).unwrap().len(), 1, "no registry trace");
    // The real device cannot rescue the session either.
    let real = phone.dev.sign_prehash(&digest).unwrap();
    let resp = fx.op(json!({"op": "enroll_ack", "signature": hex::encode(real)}));
    assert_eq!(err_code(&resp), "BAD_STATE", "{resp}");
}

/// EN-05: an ACK over the wrong head does not verify (§5.2 "ACK binds
/// head hash" — a captured session replays to nothing).
#[test]
fn ack_over_a_different_head_is_rejected() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let phone = Phone::new("ack-head");
    let begun = begin(&fx);
    let secret = begun["secret"].as_str().unwrap().to_string();
    let hello = fx.op(phone.hello(&secret, [4u8; 16]));
    let mac_id =
        hex::decode_array::<16>(hello["reply"]["mac_device_id"].as_str().unwrap()).unwrap();
    assert_eq!(fx.op(json!({"op": "enroll_confirm"}))["ok"], true);
    let bogus: [u8; 32] = Sha256::digest(b"some other registry head").into();
    let sig = phone.dev.sign_prehash(&transcript::ack_digest(&bogus, &mac_id)).unwrap();
    let resp = fx.op(json!({"op": "enroll_ack", "signature": hex::encode(sig)}));
    assert_eq!(err_code(&resp), "SIGNATURE_INVALID", "{resp}");
}

/// EN-06: §2.7 — a device presenting one key for both roles, an
/// off-curve key, or a bad platform is refused before any transcript.
#[test]
fn malformed_device_identities_are_refused() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let phone = Phone::new("malformed");
    let begun = begin(&fx);
    let secret = begun["secret"].as_str().unwrap().to_string();

    let mut same_key = phone.hello(&secret, [5u8; 16]);
    same_key["agree_pub"] = json!(hex::encode(phone.dev.sign_pub()));
    assert_eq!(err_code(&fx.op(same_key)), "INVALID_INPUT");

    let mut off_curve = phone.hello(&secret, [5u8; 16]);
    off_curve["agree_pub"] = json!(hex::encode([0x04u8; 65]));
    assert_eq!(err_code(&fx.op(off_curve)), "INVALID_INPUT");

    let mut bad_platform = phone.hello(&secret, [5u8; 16]);
    bad_platform["platform"] = json!(9);
    assert_eq!(err_code(&fx.op(bad_platform)), "INVALID_INPUT");

    let mut bad_proto = phone.hello(&secret, [5u8; 16]);
    bad_proto["proto"] = json!(1);
    assert_eq!(err_code(&fx.op(bad_proto)), "PROTOCOL_VIOLATION");

    // The session survives a malformed hello only in the sense that the
    // secret was correct; a well-formed hello still works.
    assert_eq!(fx.op(phone.hello(&secret, [5u8; 16]))["ok"], true);
}

/// EN-07: a lock in flight destroys the session (§5.3, §13.3).
#[test]
fn locking_destroys_an_in_flight_enrollment() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let phone = Phone::new("lock");
    let begun = begin(&fx);
    let secret = begun["secret"].as_str().unwrap().to_string();
    assert_eq!(fx.op(phone.hello(&secret, [6u8; 16]))["ok"], true);
    fx.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    assert_eq!(unlock(&fx, MP)["ok"], true);
    let resp = fx.op(json!({"op": "enroll_confirm"}));
    assert_eq!(err_code(&resp), "BAD_STATE", "{resp}");
}

/// EN-08: presence denial stops enrollment before anything is built.
#[test]
fn presence_denial_stops_confirmation() {
    let _g = serial();
    let fx = fx_with_presence(false);
    setup_and_unlock(&fx);
    let phone = Phone::new("presence");
    let begun = begin(&fx);
    let secret = begun["secret"].as_str().unwrap().to_string();
    assert_eq!(fx.op(phone.hello(&secret, [7u8; 16]))["ok"], true);
    let resp = fx.op(json!({"op": "enroll_confirm"}));
    assert_eq!(err_code(&resp), "PRESENCE_DENIED", "{resp}");
    assert_eq!(log::read_entries(&fx.dir).unwrap().len(), 1);
}
