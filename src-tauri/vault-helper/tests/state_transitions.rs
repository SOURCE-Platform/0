//! §13 state rules through the op surface (spec v0.4 ST-01, ST-05, BK-28
//! op contract): COMPROMISED evidence persists across lock and restart
//! (re-entered at open), reads stay allowed and writes are frozen; a
//! fully staged publication survives lock; `setup_retry_handle` runs only
//! while the first `create` is pending and commits nothing unless the new
//! Recovery Key was acknowledged. Synthetic data only.

mod vault_fx;

use vault_fx::*;
use vault_helper::errors::ErrorCode;
use vault_helper::storage::{compromised, VaultStore};
use vault_helper::vault::rk_ops::stored_handle;
use vault_helper::vault::LockReason;

fn lock(fx: &Fx) {
    fx.core.lock().unwrap().lock(LockReason::Explicit);
}

#[test]
fn st05_compromised_is_reentered_at_open() {
    let _g = serial();
    let mut fx = fx();
    setup_and_unlock(&fx);
    let r = add_login(&fx);
    lock(&fx);
    compromised::mark(&VaultStore::open(&fx.dir).unwrap().conn, ErrorCode::RegistryFork).unwrap();
    assert_eq!(state_of(&unlock(&fx, MP)), "compromised");
    assert_eq!(fx.state(), VaultState::Compromised);
    // Reads allowed…
    assert_eq!(fx.op(json!({"op": "list_items"}))["items"].as_array().unwrap().len(), 1);
    let rev = fx.op(json!({"op": "reveal", "ref": r}));
    assert_eq!(rev["ok"], true, "{rev}");
    assert_eq!(fx.state(), VaultState::Compromised, "an authorized read returns to COMPROMISED");
    // …writes frozen.
    for frame in [
        json!({"op": "add_item", "kind": "login", "title": "x", "password": "y"}),
        json!({"op": "delete_item", "ref": r}),
        json!({"op": "backup_prepare"}),
        json!({"op": "rotate_recovery_key"}),
        json!({"op": "setup_retry_handle", "handle": "synthetic-other@example.test"}),
    ] {
        assert_eq!(err_code(&fx.op(frame.clone())), "BAD_STATE", "{frame}");
    }
    // Lock, restart, unlock: still COMPROMISED.
    lock(&fx);
    assert_eq!(fx.state(), VaultState::Locked);
    fx.reboot();
    assert_eq!(state_of(&unlock(&fx, MP)), "compromised");
    fx.remove_dir();
}

#[test]
fn bk28_retry_handle_op_contract() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let generation = || VaultStore::open(&fx.dir).unwrap().header.vk_generation;
    assert_eq!(generation(), 1);
    // Invalid handle; wrong MP; a dismissed sheet — nothing commits.
    assert_eq!(err_code(&fx.op(json!({"op": "setup_retry_handle", "handle": " "}))), "INVALID_INPUT");
    fx.push_panel(submitted(b"synthetic-not-the-master-password"));
    assert_eq!(err_code(&fx.op(json!({"op": "setup_retry_handle", "handle": "synthetic-two@example.test"}))), "WRONG_CREDENTIAL");
    *fx.panel.refuse_sheet.lock().unwrap() = true;
    fx.push_panel(submitted(MP));
    assert_eq!(err_code(&fx.op(json!({"op": "setup_retry_handle", "handle": "synthetic-two@example.test"}))), "PANEL_CANCELLED");
    *fx.panel.refuse_sheet.lock().unwrap() = false;
    assert_eq!((generation(), fx.state()), (1, VaultState::Unlocked));
    // The retry: new RK acknowledged, one rotation, handle replaced, the
    // create re-staged (BACKING_UP).
    fx.push_panel(submitted(MP));
    let resp = fx.op(json!({"op": "setup_retry_handle", "handle": "Synthetic-Two@Example.test"}));
    assert_eq!(resp["ok"], true, "{resp}");
    assert_eq!(resp["publication"]["kind"], "create");
    assert_eq!(generation(), 2);
    assert_eq!(fx.core.lock().unwrap().reported_state(), VaultState::BackingUp);
    let store = VaultStore::open(&fx.dir).unwrap();
    assert_eq!(stored_handle(&store).as_deref(), Some("synthetic-two@example.test"));
    drop(store);
    // ST-01: a fully staged publication survives lock.
    lock(&fx);
    assert_eq!(fx.core.lock().unwrap().reported_state(), VaultState::BackingUp);
    // LOCKED: refused; the §11.3.2 status still reads (ciphertext-only DB).
    assert_eq!(err_code(&fx.op(json!({"op": "setup_retry_handle", "handle": "synthetic-three@example.test"}))), "BAD_STATE");
    let st = fx.op(json!({"op": "remote_update_status"}));
    assert_eq!((st["pending"].clone(), st["ops"].clone()), (json!(true), json!(["vault_create"])), "{st}");
    assert_eq!(st["security_driven"], false);
    fx.remove_dir();
}
