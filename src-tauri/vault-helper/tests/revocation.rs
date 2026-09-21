//! Phase E revocation (§11.4, §12 scenario 8, RC-01/RC-02): revoking a
//! device writes the registry entry *and* rotates the VK, so a device
//! that kept a copy of the old key cannot read anything written
//! afterwards. Synthetic vaults and credentials only.

mod device_fx;
mod vault_fx;

use device_fx::*;
use serde_json::json;
use vault_fx::*;
use vault_helper::crypto::hex;
use vault_helper::crypto::wrap::DeviceEnvelopePayload;
use vault_helper::device::identity::SeDevice;
use vault_helper::device::{envelope, se};
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::registry::log;
use vault_helper::state::VaultState;
use vault_helper::storage::header::Header;
use vault_helper::storage::store::VaultStore;

const MP_REVOKE: &[u8] = MP;

fn header_of(fx: &Fx) -> Header {
    VaultStore::read_header(&fx.dir).expect("header")
}

fn revoke(fx: &Fx, device_id: &[u8; 16]) -> serde_json::Value {
    // Presence, then the MP (to re-seal password.wrap), then the new
    // Recovery Key window the rotation forces.
    fx.push_panel(submitted(MP_REVOKE));
    fx.op(json!({"op": "revoke_device", "device_id": hex::encode(device_id)}))
}

/// RC-01: revoke → registry entry + VK rotation + re-enveloped survivors,
/// with the vault still usable and a Recovery Key that still works.
#[test]
fn revocation_rotates_the_vk_and_re_envelopes_survivors() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let item = add_login(&fx);
    let phone = Phone::new("rc01");
    let phone_id = enroll_phone(&fx, &phone);
    let before = header_of(&fx);
    assert_eq!(before.vk_generation, 1);

    let mac = SeDevice::load(&fx.dir).expect("mac identity");
    let mac_env_before = envelope::read_envelope(&fx.dir, &mac.device_id()).expect("mac envelope");

    let resp = revoke(&fx, &phone_id);
    assert_eq!(resp["ok"], true, "revoke_device: {resp}");
    assert_eq!(resp["vk_generation"], 2, "revocation must rotate the VK");

    // Registry: the revoke entry is there and the device is inactive.
    let entries = log::read_entries(&fx.dir).unwrap();
    assert_eq!(entries.len(), 3, "genesis, enroll, revoke");
    let listed = fx.op(json!({"op": "list_devices"}));
    let devices = listed["devices"].as_array().unwrap();
    let revoked = devices
        .iter()
        .find(|d| d["device_id"] == hex::encode(phone_id))
        .expect("revoked device still listed");
    assert_eq!(revoked["revoked"], true);

    // The revoked device's envelope is gone; the Mac's was re-sealed.
    assert!(envelope::read_envelope(&fx.dir, &phone_id).is_err());
    let mac_env_after = envelope::read_envelope(&fx.dir, &mac.device_id()).expect("mac envelope");
    assert_ne!(mac_env_after.ct, mac_env_before.ct, "re-sealed under the new VK");
    assert_eq!(
        mac_env_after.enrollment_nonce, mac_env_before.enrollment_nonce,
        "still bound to the same enrollment instance"
    );

    // The Mac can open its new envelope, and it carries the new VK
    // generation and the *same* backup credential (§11.4).
    let vault_id = header_of(&fx).vault_id.0;
    let opened_before =
        envelope::open_envelope(mac.key_tag(), &vault_id, &mac_env_before).expect("old envelope");
    let opened_after =
        envelope::open_envelope(mac.key_tag(), &vault_id, &mac_env_after).expect("new envelope");
    assert_eq!(opened_after.vk_generation, 2);
    assert_ne!(opened_after.vk.expose(), opened_before.vk.expose());
    assert_eq!(
        opened_after.device_backup_cred.expose(),
        opened_before.device_backup_cred.expose(),
        "device credentials rotate only by re-enrollment"
    );

    // The vault still works: the record reads, and the heads moved in
    // lockstep with the registry.
    let revealed = fx.op(json!({"op": "reveal", "ref": item}));
    assert_eq!(revealed["ok"], true, "{revealed}");
    assert!(revealed.to_string().contains(PASSWORD));
    let after = header_of(&fx);
    assert_eq!(after.registry_head.0, {
        let e = log::read_entries(&fx.dir).unwrap();
        vault_helper::crypto::registry::entry_hash(e.last().unwrap()).unwrap()
    });
    assert!(after.manifest_generation > before.manifest_generation);
    se::delete_keys(phone.dev.key_tag());
}

/// RC-02: the old VK the revoked device kept cannot open the rotated
/// state — revocation is cryptographic, not just a flag in a list.
#[test]
fn the_revoked_devices_old_key_cannot_read_new_state() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    add_login(&fx);
    let phone = Phone::new("rc02");
    let phone_id = enroll_phone(&fx, &phone);

    // What the phone took away at enrollment.
    let vault_id = header_of(&fx).vault_id.0;
    let env = envelope::read_envelope(&fx.dir, &phone_id).expect("phone envelope");
    let held: DeviceEnvelopePayload =
        envelope::open_envelope(phone.dev.key_tag(), &vault_id, &env).expect("phone opens it");
    let old_vk = vault_helper::crypto::secret::SecretBytes::new(*held.vk.expose());

    assert_eq!(revoke(&fx, &phone_id)["ok"], true);

    // Every record was re-sealed: the old VK fails the AEAD tag.
    let store = VaultStore::open(&fx.dir).expect("open");
    let rows = vault_helper::storage::revision_rows::all_rows(&store.conn).expect("rows");
    assert!(!rows.is_empty());
    for row in &rows {
        let rid = vault_helper::storage::revisions::uuid_bytes(&row.record_id).unwrap();
        let opened = vault_helper::crypto::record::open_record(
            &old_vk,
            &vault_id,
            &rid,
            row.schema_version,
            row.vk_generation,
            &vault_helper::crypto::record::RecordCiphertext {
                nonce: row.nonce,
                ct: row.ct.clone(),
            },
        );
        assert!(opened.is_err(), "old VK must not open rotated records");
    }
    se::delete_keys(phone.dev.key_tag());
}

/// A device cannot revoke itself (§4.4 rule 3: the vault would be left
/// with no authorizer), and an unknown device is NOT_FOUND.
#[test]
fn self_revocation_and_unknown_devices_are_refused() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let mac = SeDevice::load(&fx.dir).expect("mac identity");
    let resp = fx.op(json!({"op": "revoke_device", "device_id": hex::encode(mac.device_id())}));
    assert_eq!(err_code(&resp), "INVALID_INPUT", "{resp}");
    let resp = fx.op(json!({"op": "revoke_device", "device_id": hex::encode([9u8; 16])}));
    assert_eq!(err_code(&resp), "NOT_FOUND", "{resp}");
    assert_eq!(header_of(&fx).vk_generation, 1, "nothing rotated");
    assert_eq!(fx.state(), VaultState::Unlocked);
}

/// Dismissing the Recovery Key window aborts the revocation: no registry
/// entry, no rotation — the user never ends up with a vault whose
/// Recovery Key they have not seen (§5.4 applied to rotation).
#[test]
fn a_dismissed_recovery_key_window_aborts_the_revocation() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let phone = Phone::new("rc-cancel");
    let phone_id = enroll_phone(&fx, &phone);
    *fx.panel.refuse_sheet.lock().unwrap() = true;
    let resp = revoke(&fx, &phone_id);
    assert_eq!(err_code(&resp), "PANEL_CANCELLED", "{resp}");
    assert_eq!(log::read_entries(&fx.dir).unwrap().len(), 2, "no revoke entry");
    assert_eq!(header_of(&fx).vk_generation, 1, "no rotation");
    assert!(envelope::read_envelope(&fx.dir, &phone_id).is_ok());
    se::delete_keys(phone.dev.key_tag());
}

/// The wrong master password stops the revocation before anything is
/// written, and the new Recovery Key is never shown.
#[test]
fn a_wrong_master_password_stops_the_revocation() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let phone = Phone::new("rc-mp");
    let phone_id = enroll_phone(&fx, &phone);
    let sheets = fx.panel.sheets.lock().unwrap().len();
    fx.push_panel(submitted(b"synthetic-wrong-master-password"));
    let resp = fx.op(json!({"op": "revoke_device", "device_id": hex::encode(phone_id)}));
    assert_eq!(err_code(&resp), "WRONG_CREDENTIAL", "{resp}");
    assert_eq!(log::read_entries(&fx.dir).unwrap().len(), 2);
    assert_eq!(header_of(&fx).vk_generation, 1);
    assert_eq!(
        fx.panel.sheets.lock().unwrap().len(),
        sheets,
        "no Recovery Key shown for a failed revocation"
    );
    se::delete_keys(phone.dev.key_tag());
}
