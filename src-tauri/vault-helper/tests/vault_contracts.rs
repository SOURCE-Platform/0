//! Op-surface contract tests: the OP-04 bad-state matrix, the SC-01/SC-04
//! frame contracts (no credential-bearing wire fields), panel-request
//! flow kinds (§1.7), and the Phase C auto-lock minutes op.

mod vault_fx;

use vault_fx::*;

// --- OP-04: bad-state matrix ---------------------------------------------------

#[test]
fn op04_bad_state_matrix() {
    let _g = serial();
    let fx = fx(); // UNINITIALIZED
    for frame in [
        json!({"op": "list_items"}),
        json!({"op": "add_item", "kind": "login", "title": "x", "password": "y"}),
        json!({"op": "update_item", "ref": "r"}),
        json!({"op": "delete_item", "ref": "r"}),
        json!({"op": "reveal", "ref": "r"}),
        json!({"op": "resolve_conflict", "ref": "r", "chosen_rev": "00".repeat(32)}),
        json!({"op": "change_master_password"}),
        json!({"op": "begin_recovery_unlock", "kind": "mp"}),
        json!({"op": "begin_recovery_unlock", "kind": "rk"}),
        json!({"op": "rotate_recovery_key"}),
        json!({"op": "change_master_password", "mode": "reset"}),
        json!({"op": "set_auto_lock_minutes", "minutes": 15}),
    ] {
        let resp = fx.op(frame.clone());
        assert_eq!(
            err_code(&resp),
            "BAD_STATE",
            "{frame} in UNINITIALIZED: {resp}"
        );
    }
    // setup while LOCKED is rejected; unknown op / unknown kind per spec.
    assert_eq!(setup_vault(&fx, MP)["ok"], true);
    let resp = fx.op(json!({"op": "setup_vault"}));
    assert_eq!(err_code(&resp), "BAD_STATE");
    let resp = fx.op(json!({"op": "nuke_everything"}));
    assert_eq!(err_code(&resp), "UNKNOWN_OP");
    let resp = fx.op(json!({"op": "begin_recovery_unlock", "kind": "device"}));
    assert_eq!(err_code(&resp), "UNKNOWN_OP");
    // Unlocked-only Phase D ops are refused while LOCKED.
    for frame in [json!({"op": "rotate_recovery_key"}), json!({"op": "change_master_password", "mode": "reset"})] {
        assert_eq!(err_code(&fx.op(frame.clone())), "BAD_STATE", "{frame} in LOCKED");
    }
    let resp = fx.op(json!({"op": "begin_recovery_unlock"}));
    assert_eq!(err_code(&resp), "INVALID_INPUT");
    fx.remove_dir();
}

// --- SC-01/SC-04: no credential-bearing wire fields ----------------------------

#[test]
fn sc01_sc04_op_surface_has_no_credential_fields() {
    let _g = serial();
    let fx = fx();
    // A frame claiming to carry the MP must not be consumed: the op still
    // drives the panel (empty queue → PANEL_CANCELLED proves it).
    let resp = fx.op(json!({"op": "setup_vault", "master_password": "attacker-supplied"}));
    assert_eq!(err_code(&resp), "PANEL_CANCELLED", "{resp}");
    assert_eq!(fx.panel.seen.lock().unwrap().len(), 1);

    assert_eq!(setup_vault(&fx, MP)["ok"], true);
    assert_eq!(unlock(&fx, MP)["ok"], true);

    // Forbidden plaintext fields are rejected before anything is written
    // (§8.1 whitelist: cvv/pin and friends never enter the vault).
    let resp = fx.op(json!({
        "op": "add_item", "kind": "card", "label": "x", "number": "4111111111111111",
        "expiry": "12/30", "cvv": "123",
    }));
    assert_eq!(err_code(&resp), "INVALID_INPUT", "{resp}");
    let resp = fx.op(json!({"op": "list_items"}));
    assert_eq!(resp["items"].as_array().unwrap().len(), 0);

    // Response contract: every op answer's keys stay within the schema;
    // secrets appear only under reveal's `secret` object.
    let r = add_login(&fx);
    let reveal = fx.op(json!({"op": "reveal", "ref": r}));
    assert_eq!(reveal["ok"], true);
    let keys: Vec<&str> = reveal
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    for k in &keys {
        assert!(
            ["ok", "error", "ref", "kind", "secret"].contains(k),
            "unexpected response key {k}"
        );
    }
    let secret_keys: Vec<&str> = reveal["secret"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(secret_keys, ["password"]);
    // The MP never appears in ANY response across the whole flow.
    for e in fx.events.log.lock().unwrap().iter() {
        assert!(!e.to_string().contains("synthetic-master-password"));
    }
    fx.remove_dir();
}

// --- auto-lock minutes op -------------------------------------------------------

#[test]
fn auto_lock_minutes_op_validates_range_and_persists() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let resp = fx.op(json!({"op": "set_auto_lock_minutes", "minutes": 5}));
    assert_eq!(resp["ok"], true, "{resp}");
    assert_eq!(fx.core.lock().unwrap().auto_lock_minutes, 5);
    assert_eq!(keychain::read_auto_lock_minutes(), 5);
    for bad in [4u64, 61, 0] {
        let resp = fx.op(json!({"op": "set_auto_lock_minutes", "minutes": bad}));
        assert_eq!(err_code(&resp), "INVALID_INPUT", "{bad}");
    }
    fx.remove_dir();
}

/// Panel requests match the op flows (§1.7): create on setup, entry on
/// unlock, change triple on change_master_password.
#[test]
fn panel_requests_follow_flow_kinds() {
    let _g = serial();
    let fx = fx();
    assert_eq!(setup_vault(&fx, MP)["ok"], true);
    assert_eq!(unlock(&fx, MP)["ok"], true);
    fx.push_panel(PanelOutcome::SubmittedChange(
        SecretVec::new(MP.to_vec()),
        SecretVec::new(MP_NEW.to_vec()),
    ));
    assert_eq!(fx.op(json!({"op": "change_master_password"}))["ok"], true);
    let seen = fx.panel.seen.lock().unwrap();
    assert_eq!(
        *seen,
        vec![
            PanelRequest::MpCreate,
            PanelRequest::MpEntry,
            PanelRequest::MpChange
        ]
    );
    fx.remove_dir();
}
