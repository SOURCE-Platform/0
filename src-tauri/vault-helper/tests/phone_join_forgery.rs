//! Forgeries a relaying main process could attempt against a joining phone
//! (spec §5.2, §22.10; F.2b re-review): a hello reply that does not open
//! the QR's commitment, an envelope beside the signed index that really
//! seals a key of its own with a checkpoint under that key, a wrap changed
//! under its hash, a bad QR, a foreign session id. Synthetic data only.

mod device_fx;
mod join_fx;
mod vault_fx;

use join_fx::*;
use serde_json::{json, Value};
use vault_fx::*;
use vault_helper::backup::checkpoint::RegistryCheckpoint;
use vault_helper::backup::index::{ObjectIndex, Role};
use vault_helper::backup::manifest::SignedManifest;
use vault_helper::crypto::hex;
use vault_helper::crypto::secret::SecretBytes;
use vault_helper::crypto::wrap::DeviceEnvelopePayload;
use vault_helper::device::SeDevice;
use vault_helper::registry::device::DeviceIdentity;

/// SEC-B1 (0f5f21b): the Mac's half of the code is committed in the QR —
/// a reply naming another device id does not open it.
#[test]
fn a_reply_that_does_not_open_the_commitment_is_refused() {
    let _g = serial();
    let mac = fx();
    setup_and_unlock(&mac);
    let phone = fx();
    let begun = device_fx::begin(&mac);
    let started = phone.op(json!({"op": "join_begin", "qr": qr(&begun), "name": "Synthetic iPhone"}));
    let mut hello = started["hello"].clone();
    hello["op"] = json!("enroll_hello");
    let mut reply = mac.op(hello)["reply"].clone();
    reply["new_device_id"] = json!("ab".repeat(16));
    assert_eq!(phone.op(json!({"op": "join_hello", "reply": reply}))["error"], "PROTOCOL_VIOLATION");
    assert_eq!(phone.op(json!({"op": "join_abort"}))["ok"], true);
    nothing_left(&phone);
    mac.remove_dir();
    phone.remove_dir();
}

/// VER-I1: an envelope that really opens — a key of the relay's own,
/// sealed to this phone for this enrollment, with a checkpoint under that
/// key — is refused because it is not the copy the signed index names.
#[test]
fn a_valid_envelope_beside_the_index_is_never_opened() {
    let _g = serial();
    let mac = fx();
    setup_and_unlock(&mac);
    let phone = fx();
    let (mut bundle, _) = pair_up_to_bundle(&mac, &phone, |_| {});
    let me = SeDevice::load(&phone.dir).unwrap();
    let vault_id = hex::decode_array::<16>(bundle["vault_id"].as_str().unwrap()).unwrap();
    let nonce = hex::decode_array::<16>(bundle["envelope"]["enrollment_nonce"].as_str().unwrap()).unwrap();
    let manifest = SignedManifest::decode(&hex::decode(bundle["manifest"].as_str().unwrap()).unwrap()).unwrap();
    let entries = vault_helper::registry::file::decode(&hex::decode(bundle["registry"].as_str().unwrap()).unwrap()).unwrap();
    let rogue = SecretBytes::new([0x5E; 32]);
    let payload = DeviceEnvelopePayload { vk: SecretBytes::new(*rogue.expose()), wrapped_at: 1, vk_generation: manifest.vk_generation };
    let env = vault_helper::device::envelope::seal_envelope(&me.agree_pub(), &vault_id, &me.device_id(), &nonce, &payload).unwrap();
    bundle["envelope"] = serde_json::to_value(&env).unwrap();
    let cp = RegistryCheckpoint::create(&rogue, &manifest, entries.last().unwrap().epoch).unwrap();
    bundle["checkpoint"] = json!(hex::encode(cp.encode()));
    let done = complete(&phone, &serde_json::to_vec(&bundle).unwrap());
    assert_eq!(done["error"], "WRAP_CORRUPT", "{done}");
    nothing_left(&phone);
    mac.remove_dir();
    phone.remove_dir();
}

/// VER-I2: the master-password wrap is written without being parsed — its
/// hash is its only guard.
#[test]
fn the_password_wrap_changed_under_its_hash_is_refused() {
    refused("wrap", |b| {
        let manifest = SignedManifest::decode(&hex::decode(b["manifest"].as_str().unwrap()).unwrap()).unwrap();
        let objects = b["objects"].as_array_mut().unwrap();
        let find = |objects: &Vec<Value>, key: &str| objects.iter().position(|o| o[0] == key).unwrap();
        let index_at = find(objects, &hex::encode(manifest.object_index_hash));
        let index = ObjectIndex::decode(&hex::decode(objects[index_at][1].as_str().unwrap()).unwrap()).unwrap();
        let wrap = hex::encode(index.find(&Role::WrapMp).unwrap().blob);
        let at = find(objects, &wrap);
        let data = objects[at][1].as_str().unwrap().to_string();
        let flipped = format!("{}{}", &data[..data.len() - 1], if data.ends_with('0') { '1' } else { '0' });
        objects[at][1] = json!(flipped);
    });
}

/// VER-O8: a bad QR creates no keys; a foreign session id leaves the real
/// join session in place.
#[test]
fn a_bad_qr_creates_nothing_and_a_foreign_session_is_refused() {
    let _g = serial();
    let mac = fx();
    setup_and_unlock(&mac);
    let phone = fx();
    let begun = device_fx::begin(&mac);
    let mut bad = qr(&begun);
    bad["mac_key"] = json!("not hex");
    assert_eq!(phone.op(json!({"op": "join_begin", "qr": bad, "name": "Synthetic iPhone"}))["ok"], false);
    nothing_left(&phone);
    let (bundle, _) = pair_up_to_bundle(&mac, &phone, |_| {});
    assert_eq!(phone.op(json!({"op": "join_complete", "session": "00".repeat(16)}))["ok"], false);
    let done = complete(&phone, &serde_json::to_vec(&bundle).unwrap());
    assert_eq!(done["ok"], true, "the real session survived: {done}");
    assert_eq!(phone.op(json!({"op": "join_abort"}))["ok"], true);
    nothing_left(&phone);
    mac.remove_dir();
    phone.remove_dir();
}
