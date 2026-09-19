//! Phase D IPC ops through `vault::dispatch` (spec §1.5, §1.7, §12
//! scenarios 5–7): RK shown at setup, RK unlock, `rotate_recovery_key`,
//! forgotten-MP reset — and the UI-05 audit that no response or event
//! ever carries MP bytes or Recovery Key words. Stub panels; production
//! Argon2id tuple; synthetic credentials only; nothing secret printed.

mod vault_fx;

use serde_json::{json, Value};
use vault_fx::*;
use vault_helper::state::VaultState;
use vault_helper::storage::store::RECOVERY_WRAP_NAME;
use vault_helper::vault::LockReason;

/// `lock` is applied by the connection layer, not `dispatch` (§13.3).
fn lock(fx: &Fx) {
    let events = fx.core.lock().unwrap().lock(LockReason::Explicit);
    for e in events {
        fx.events.log.lock().unwrap().push(e);
    }
}

/// Every frame the helper produced (responses + events) as one string.
fn transcript(fx: &Fx, responses: &[Value]) -> String {
    let mut all: Vec<String> = responses.iter().map(Value::to_string).collect();
    all.extend(fx.events.log.lock().unwrap().iter().map(Value::to_string));
    all.join("\n")
}

/// UI-05: no MP bytes, no RK word sequence, in anything that crossed.
fn assert_no_secrets(fx: &Fx, responses: &[Value], mps: &[&[u8]]) {
    let t = transcript(fx, responses);
    for mp in mps {
        assert!(!t.contains(std::str::from_utf8(mp).unwrap()), "MP crossed IPC");
    }
    if let Some(words) = fx.panel.shown_rk.lock().unwrap().as_ref() {
        let first_three: Vec<&str> = words.split_whitespace().take(3).collect();
        assert!(!t.contains(&first_three.join(" ")), "RK words crossed IPC");
        assert!(!t.contains(words.as_str()));
    }
    for banned in ["\"words\"", "\"mnemonic\"", "\"recovery_key\"", "\"rk\":", "\"vk\""] {
        assert!(!t.contains(banned), "field {banned} crossed IPC");
    }
}

#[test]
fn setup_shows_rk_and_rk_unlock_works() {
    let _g = serial();
    let fx = fx();
    let r1 = setup_vault(&fx, MP);
    assert_eq!(r1["ok"], true, "{r1}");
    assert!(fx.dir.join(RECOVERY_WRAP_NAME).exists(), "RK wrap written at setup (§5.4)");
    let sheets = fx.panel.sheets.lock().unwrap().clone();
    assert_eq!(sheets.len(), 1, "RK shown exactly once at setup");
    assert!(sheets[0].contains("generation 1"), "sheet carries the §11.7 checkpoint");
    // The RK window is bracketed by the capture-suppression event with its title.
    let titles: Vec<Value> = fx.events_named("secure_panel_visible").iter().map(|e| e["title"].clone()).collect();
    assert!(titles.contains(&json!("Source Vault — Recovery Key")));
    let panels = fx.events_named("secure_panel_visible");
    assert_eq!(panels.iter().filter(|e| e["visible"] == true).count(), panels.iter().filter(|e| e["visible"] == false).count());

    // Unlock with the RK (stub panel types back the words it was shown).
    let r2 = fx.op(json!({"op": "begin_recovery_unlock", "kind": "rk"}));
    assert_eq!(r2["ok"], true, "{r2}");
    assert_eq!(fx.state(), VaultState::Unlocked);
    // A frame claiming to carry words is ignored — only the panel counts.
    lock(&fx);
    *fx.panel.shown_rk.lock().unwrap() = None;
    let r3 = fx.op(json!({"op": "begin_recovery_unlock", "kind": "rk", "words": "abandon abandon"}));
    assert_eq!(err_code(&r3), "PANEL_CANCELLED", "{r3}");
    assert_no_secrets(&fx, &[r1, r2, r3], &[MP]);
}

#[test]
fn bad_rk_is_recovery_key_invalid_without_oracle() {
    let _g = serial();
    let fx = fx();
    setup_vault(&fx, MP);
    // Right word count, wrong checksum.
    *fx.panel.shown_rk.lock().unwrap() = Some(["abandon"; 24].join(" "));
    let r = fx.op(json!({"op": "begin_recovery_unlock", "kind": "rk"}));
    assert_eq!(err_code(&r), "RECOVERY_KEY_INVALID", "{r}");
    assert_eq!(fx.state(), VaultState::Locked);
}

#[test]
fn refused_rk_sheet_at_setup_leaves_no_vault() {
    let _g = serial();
    let fx = fx();
    *fx.panel.refuse_sheet.lock().unwrap() = true;
    let r = setup_vault(&fx, MP);
    assert_eq!(err_code(&r), "PANEL_CANCELLED", "{r}");
    assert!(!fx.dir.join("header.json").exists() && !fx.dir.join(RECOVERY_WRAP_NAME).exists());
}

#[test]
fn rotate_recovery_key_op() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let reference = add_login(&fx);
    let old_rk = fx.panel.shown_rk.lock().unwrap().clone().unwrap();
    // Refused sheet → nothing committed, old RK still unlocks.
    *fx.panel.refuse_sheet.lock().unwrap() = true;
    fx.push_panel(submitted(MP));
    let r = fx.op(json!({"op": "rotate_recovery_key"}));
    assert_eq!(err_code(&r), "PANEL_CANCELLED", "{r}");
    *fx.panel.refuse_sheet.lock().unwrap() = false;
    // Wrong current MP → WRONG_CREDENTIAL, nothing shown.
    let before = fx.panel.sheets.lock().unwrap().len();
    fx.push_panel(submitted(b"synthetic-not-the-password"));
    assert_eq!(err_code(&fx.op(json!({"op": "rotate_recovery_key"}))), "WRONG_CREDENTIAL");
    assert_eq!(fx.panel.sheets.lock().unwrap().len(), before);
    // Success: new RK shown, VK rotated, data kept, back to UNLOCKED.
    fx.push_panel(submitted(MP));
    let ok = fx.op(json!({"op": "rotate_recovery_key"}));
    assert_eq!(ok["ok"], true, "{ok}");
    assert_eq!(fx.state(), VaultState::Unlocked);
    let new_rk = fx.panel.shown_rk.lock().unwrap().clone().unwrap();
    assert_ne!(new_rk, old_rk);
    let rev = fx.op(json!({"op": "reveal", "ref": reference}));
    assert_eq!(rev["ok"], true, "record survives rotation: {rev}");
    // Old RK fails, new RK and MP unlock.
    lock(&fx);
    *fx.panel.shown_rk.lock().unwrap() = Some(old_rk);
    assert_eq!(err_code(&fx.op(json!({"op": "begin_recovery_unlock", "kind": "rk"}))), "WRONG_CREDENTIAL");
    *fx.panel.shown_rk.lock().unwrap() = Some(new_rk);
    assert_eq!(fx.op(json!({"op": "begin_recovery_unlock", "kind": "rk"}))["ok"], true);
    lock(&fx);
    assert_eq!(unlock(&fx, MP)["ok"], true);
    assert_no_secrets(&fx, &[ok, rev], &[MP]);
}

#[test]
fn forgotten_mp_reset_after_rk_unlock() {
    let _g = serial();
    let fx = fx();
    setup_vault(&fx, MP);
    assert_eq!(fx.op(json!({"op": "begin_recovery_unlock", "kind": "rk"}))["ok"], true);
    fx.push_panel(submitted(MP_NEW));
    let r = fx.op(json!({"op": "change_master_password", "mode": "reset"}));
    assert_eq!(r["ok"], true, "{r}");
    lock(&fx);
    assert_eq!(err_code(&unlock(&fx, MP)), "WRONG_CREDENTIAL", "old MP dead");
    assert_eq!(unlock(&fx, MP_NEW)["ok"], true);
    assert_no_secrets(&fx, &[r], &[MP, MP_NEW]);
}
