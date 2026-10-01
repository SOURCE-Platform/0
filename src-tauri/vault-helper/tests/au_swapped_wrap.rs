//! §22.4 (F2-D3), review SEC-B1: a `password.wrap` replaced on disk with
//! one sealed under a password the attacker chose must not pass any
//! master-password gate — the proof is bound to the resident VK. The
//! attacker here holds the login password (presence passes) and can
//! write the vault directory. Synthetic data only.

mod vault_fx;

use serde_json::json;
use vault_fx::*;
use vault_helper::crypto::kdf::{derive_pk, Argon2Params};
use vault_helper::crypto::secret::{random_salt, random_secret, SecretBytes};
use vault_helper::crypto::wrap::{seal_wrap_mp, seal_wrap_rk, RecoveryWrapPayload};

const CHOSEN: &[u8] = b"attacker-chosen-password (synthetic)";

fn swap_password_wrap(fx: &Fx) {
    let header = vault_helper::storage::VaultStore::read_header(&fx.dir).unwrap();
    let salt = random_salt();
    let pk = derive_pk(CHOSEN, &salt, Argon2Params::V1).unwrap();
    let other: SecretBytes<32> = random_secret();
    let payload = RecoveryWrapPayload { vk: other, wrapped_at: 1, vk_generation: header.vk_generation };
    let file = seal_wrap_mp(&payload, &pk, &header.vault_id.0, Argon2Params::V1, &salt).unwrap();
    std::fs::write(fx.dir.join(PASSWORD_WRAP_NAME), serde_json::to_vec_pretty(&file).unwrap()).unwrap();
}

#[test]
fn a_replaced_password_wrap_passes_no_gate() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let refs: Vec<String> = (0..11).map(|_| add_login(&fx)).collect();
    swap_password_wrap(&fx);

    // Change MP with the chosen password as "old".
    fx.push_panel(PanelOutcome::SubmittedChange(SecretVec::new(CHOSEN.to_vec()), SecretVec::new(MP_NEW.to_vec())));
    assert_eq!(err_code(&fx.op(json!({"op": "change_master_password"}))), "WRONG_CREDENTIAL");

    // Rotate the RK with the chosen password.
    fx.push_panel(submitted(CHOSEN));
    assert_eq!(err_code(&fx.op(json!({"op": "rotate_recovery_key"}))), "WRONG_CREDENTIAL");

    // Bulk deletion past the threshold with the chosen password.
    for r in &refs[..10] {
        assert_eq!(fx.op(json!({"op": "delete_item", "ref": r}))["ok"], true);
    }
    fx.push_panel(submitted(CHOSEN));
    assert_eq!(err_code(&fx.op(json!({"op": "delete_item", "ref": refs[10]}))), "WRONG_CREDENTIAL");
    assert_eq!(fx.op(json!({"op": "list_items"}))["items"].as_array().unwrap().len(), 1);
    assert_eq!(fx.state(), VaultState::Unlocked);
    fx.remove_dir();
}

/// VER-I5: the Recovery Key proof is bound to the resident VK too — a
/// `recovery.wrap` sealed under the REAL Recovery Key but over some other
/// key (e.g. a stale or planted wrap) does not reset the master password.
#[test]
fn a_recovery_wrap_over_another_key_does_not_reset_the_mp() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let words = fx.panel.shown_rk.lock().unwrap().clone().unwrap();
    let rk = vault_helper::crypto::bip39::decode_rk(&words).unwrap();
    let header = vault_helper::storage::VaultStore::read_header(&fx.dir).unwrap();
    let other: SecretBytes<32> = random_secret();
    let payload = RecoveryWrapPayload { vk: other, wrapped_at: 1, vk_generation: header.vk_generation };
    let file = seal_wrap_rk(&payload, &rk, &header.vault_id.0).unwrap();
    std::fs::write(fx.dir.join(vault_helper::storage::store::RECOVERY_WRAP_NAME), serde_json::to_vec_pretty(&file).unwrap()).unwrap();
    fx.push_panel(submitted(MP_NEW));
    assert_eq!(err_code(&fx.op(json!({"op": "change_master_password", "mode": "reset"}))), "WRONG_CREDENTIAL");
    fx.panel.queue.lock().unwrap().clear();
    fx.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    assert_eq!(unlock(&fx, MP)["ok"], true, "the master password is unchanged");
    fx.remove_dir();
}

/// VER-B3: unlocking THROUGH a planted wrap must fail — otherwise the
/// planted key becomes resident and every later gate compares against
/// it. The key must match the commitment this Mac's Secure Enclave
/// signed. The same holds for a planted Recovery Key wrap and a planted
/// device envelope (anyone can seal to the public agreement key).
#[test]
fn unlocking_through_a_planted_key_is_refused() {
    use vault_helper::device::{envelope, SeDevice};
    use vault_helper::registry::device::DeviceIdentity;
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    add_login(&fx);
    fx.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    let real_wrap = std::fs::read(fx.dir.join(PASSWORD_WRAP_NAME)).unwrap();
    swap_password_wrap(&fx);
    assert_eq!(err_code(&unlock(&fx, CHOSEN)), "WRONG_CREDENTIAL");
    assert_eq!(fx.state(), VaultState::Locked, "the planted key never became resident");

    // A planted device envelope over another key.
    std::fs::write(fx.dir.join(PASSWORD_WRAP_NAME), &real_wrap).unwrap();
    let header = vault_helper::storage::VaultStore::read_header(&fx.dir).unwrap();
    let dev = SeDevice::load(&fx.dir).unwrap();
    let payload = vault_helper::crypto::wrap::DeviceEnvelopePayload { vk: random_secret(), wrapped_at: 1, vk_generation: header.vk_generation };
    let env = envelope::seal_envelope(&dev.agree_pub(), &header.vault_id.0, &dev.device_id(), &[7u8; 16], &payload).unwrap();
    envelope::write_envelope(&fx.dir, &dev.device_id(), &env).unwrap();
    assert_eq!(err_code(&fx.op(json!({"op": "unlock"}))), "WRONG_CREDENTIAL");
    assert_eq!(fx.state(), VaultState::Locked);

    // The real master password still opens the real key.
    assert_eq!(unlock(&fx, MP)["ok"], true);
    assert_eq!(fx.op(json!({"op": "list_items"}))["items"].as_array().unwrap().len(), 1);
    fx.remove_dir();
}
