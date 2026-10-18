//! AU-05 (§22.4): a stolen Mac with its login password — presence passes,
//! the master password is unknown. Reads work (the stated residual); no
//! device removal, Recovery Key change, master-password change or reset.
//! A paired iPhone exists, so the removal has a real target. Synthetic.

mod device_fx;
mod join_fx;
mod sync_fx;
mod vault_fx;

use serde_json::json;
use sync_fx::*;
use vault_fx::*;
use vault_helper::registry::device::DeviceIdentity;

#[test]
fn au05_the_login_password_alone_carries_no_authority() {
    let _g = serial();
    let (mac, phone, item) = paired();
    assert_eq!(mac.op(json!({"op": "reveal", "ref": item}))["ok"], true, "reads: the stated residual");
    let guess = b"synthetic-attacker-guess";
    let phone_id = vault_helper::crypto::hex::encode(device(&phone).device_id());
    for frame in [json!({"op": "revoke_device", "device_id": phone_id}), json!({"op": "rotate_recovery_key"})] {
        mac.push_panel(submitted(guess));
        assert_eq!(err_code(&mac.op(frame.clone())), "WRONG_CREDENTIAL", "{frame}");
    }
    mac.push_panel(PanelOutcome::SubmittedChange(SecretVec::new(guess.to_vec()), SecretVec::new(MP_NEW.to_vec())));
    assert_eq!(err_code(&mac.op(json!({"op": "change_master_password"}))), "WRONG_CREDENTIAL");
    // The stub panel types the shown Recovery Key by itself; the thief has
    // never seen it.
    *mac.panel.shown_rk.lock().unwrap() = None;
    mac.push_panel(submitted(b"not a recovery key"));
    assert_ne!(mac.op(json!({"op": "change_master_password", "mode": "reset"}))["ok"], true, "reset needs the Recovery Key");
    mac.panel.queue.lock().unwrap().clear();
    let devices = mac.op(json!({"op": "list_devices"}));
    assert!(devices["devices"].as_array().unwrap().iter().all(|d| d["revoked"] == false), "nobody was removed: {devices}");
    mac.remove_dir();
    phone.remove_dir();
}
