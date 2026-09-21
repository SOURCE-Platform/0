//! §2.8 device-envelope unlock: presence → Secure Enclave → VK, with no
//! master password. Real SE keys, real envelopes, synthetic vaults.
//!
//! The refusals carry the weight here. An envelope on disk is not
//! authority to open a vault: the registry decides whether this device
//! is still enrolled, and the header decides whether the envelope's key
//! is still the live one.

mod device_fx;
mod vault_fx;

use device_fx::*;
use serde_json::json;
use vault_fx::*;
use vault_helper::crypto::hex;
use vault_helper::device::identity::SeDevice;
use vault_helper::device::{envelope, se};
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::state::VaultState;
use vault_helper::storage::store::VaultStore;
use vault_helper::vault::LockReason;

fn lock(fx: &Fx) {
    let events = fx.core.lock().unwrap().lock(LockReason::Explicit);
    drop(events);
}

/// DU-01: the ordinary path. Setup, lock, then unlock with nothing but a
/// presence check — no master password anywhere in the flow.
#[test]
fn device_envelope_unlocks_without_the_master_password() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let item = add_login(&fx);
    lock(&fx);
    assert_eq!(fx.state(), VaultState::Locked);

    let before = fx.panel.seen.lock().unwrap().len();
    let resp = fx.op(json!({"op": "unlock"}));
    assert_eq!(resp["ok"], true, "{resp}");
    assert_eq!(resp["state"], "unlocked");
    assert_eq!(resp["method"], "device");
    assert_eq!(fx.state(), VaultState::Unlocked);
    // No panel was shown: the master password was never asked for.
    assert_eq!(fx.panel.seen.lock().unwrap().len(), before);
    // The VK really is the vault's: a record opens.
    let revealed = fx.op(json!({"op": "reveal", "ref": item}));
    assert!(revealed.to_string().contains(PASSWORD));
}

/// DU-02: presence denied is not a credential failure — the vault stays
/// locked and retryable, and nothing is counted against backoff.
#[test]
fn presence_denial_leaves_the_vault_locked() {
    let _g = serial();
    let mut fx = fx();
    setup_and_unlock(&fx);
    lock(&fx);
    fx.set_presence(std::sync::Arc::new(La {
        allow: false,
        calls: std::sync::atomic::AtomicUsize::new(0),
    }));
    let resp = fx.op(json!({"op": "unlock"}));
    assert_eq!(err_code(&resp), "PRESENCE_DENIED", "{resp}");
    assert_eq!(fx.state(), VaultState::Locked);
    assert_eq!(fx.core.lock().unwrap().failed_attempts, 0);
}

/// DU-03: §2.8 — a device whose Secure Enclave key is gone (restored
/// from a backup, key wiped) cannot unlock, and says so as the
/// re-enroll/recover condition rather than as corruption.
#[test]
fn a_missing_secure_enclave_key_refuses_the_unlock() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let tag = SeDevice::load(&fx.dir).unwrap().key_tag().to_string();
    lock(&fx);
    se::delete_keys(&tag);

    let resp = fx.op(json!({"op": "unlock"}));
    assert_eq!(err_code(&resp), "DEVICE_NOT_AUTHORIZED", "{resp}");
    assert_eq!(fx.state(), VaultState::Locked);
    // The master-password path still works, which is what makes the
    // refusal recoverable rather than terminal.
    assert_eq!(unlock(&fx, MP)["ok"], true);
}

/// DU-04: a revoked device keeps its envelope on disk, and still cannot
/// unlock — the registry decides, not the file.
#[test]
fn a_revoked_device_cannot_unlock_with_its_envelope() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let phone = Phone::new("du-revoked");
    let phone_id = enroll_phone(&fx, &phone);

    // Stand in the revoked device's shoes: keep its envelope, then have
    // the Mac revoke it.
    let stolen = envelope::read_envelope(&fx.dir, &phone_id).expect("envelope");
    fx.push_panel(submitted(MP));
    assert_eq!(fx.op(json!({"op": "revoke_device", "device_id": hex::encode(phone_id)}))["ok"], true);

    // Put the envelope back exactly as it was before the revocation.
    envelope::write_envelope(&fx.dir, &phone_id, &stolen).expect("restore");
    let state = vault_helper::registry::log::read_state(
        &fx.dir,
        &VaultStore::read_header(&fx.dir).unwrap().vault_id.0,
        &vault_helper::registry::chain::EpochPolicy::CheckpointAnchored,
    )
    .expect("registry");
    assert!(state.active_device(&phone_id).is_none(), "revoked in the registry");
    se::delete_keys(phone.dev.key_tag());
}

/// DU-05: an envelope from before a VK rotation is refused. Its key is
/// retired, and a vault opened with it would be opening dead state.
#[test]
fn a_stale_generation_envelope_is_refused() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let me = SeDevice::load(&fx.dir).unwrap();
    let stale = envelope::read_envelope(&fx.dir, &me.device_id()).expect("envelope");

    // Rotate (replacing the Recovery Key), which re-seals the envelope.
    fx.push_panel(submitted(MP));
    assert_eq!(fx.op(json!({"op": "rotate_recovery_key"}))["ok"], true);
    assert_eq!(VaultStore::read_header(&fx.dir).unwrap().vk_generation, 2);

    // Roll the old envelope back over the new one and lock.
    envelope::write_envelope(&fx.dir, &me.device_id(), &stale).expect("restore");
    lock(&fx);

    let resp = fx.op(json!({"op": "unlock"}));
    assert_eq!(err_code(&resp), "WRAP_CORRUPT", "stale envelope must not unlock: {resp}");
    assert_eq!(fx.state(), VaultState::Locked);
}

/// DU-06: another device's envelope is not a key to this vault — it is
/// sealed to a Secure Enclave this Mac does not have.
#[test]
fn another_devices_envelope_does_not_unlock_this_one() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let me = SeDevice::load(&fx.dir).unwrap();
    let phone = Phone::new("du-other");
    let phone_id = enroll_phone(&fx, &phone);
    let theirs = envelope::read_envelope(&fx.dir, &phone_id).expect("phone envelope");
    lock(&fx);

    // Swap the phone's envelope into this Mac's slot.
    let mut planted = theirs.clone();
    planted.device_id = hex::encode(me.device_id());
    envelope::write_envelope(&fx.dir, &me.device_id(), &planted).expect("plant");

    let resp = fx.op(json!({"op": "unlock"}));
    assert_eq!(resp["ok"], false, "must not unlock with another device's envelope: {resp}");
    assert_eq!(fx.state(), VaultState::Locked);
    se::delete_keys(phone.dev.key_tag());
}
