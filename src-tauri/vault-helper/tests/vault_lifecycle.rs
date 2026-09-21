//! Op-level vault lifecycle tests OP-01…OP-03, OP-05…OP-07. Numbered in
//! their own series: the spec's CS-xx IDs are capture-suppression tests
//! (§16), mapped separately in docs/security/phase-c-verification.md.
//! Every op runs through `vault::dispatch` exactly as the executor invokes
//! it, against a real on-disk vault and stub Deps (see `vault_fx`).

mod vault_fx;

use vault_fx::*;

// --- OP-01: full lifecycle ---------------------------------------------------

#[test]
fn op01_setup_unlock_crud_reveal_lifecycle() {
    let _g = serial();
    let fx = fx();
    assert_eq!(fx.state(), VaultState::Uninitialized);

    // setup_vault: panel gets MpCreate, vault files appear, state LOCKED.
    let resp = setup_vault(&fx, MP);
    assert_eq!(resp["ok"], true, "{resp}");
    assert_eq!(state_of(&resp), "locked");
    assert_eq!(fx.state(), VaultState::Locked);
    for name in [VAULT_HEADER_NAME, "vault.db", MANIFEST_NAME, PASSWORD_WRAP_NAME] {
        assert!(fx.dir.join(name).exists(), "missing {name}");
    }
    // §14.2: the panel-visibility events bracket the panel with its title.
    // Phase D: the Recovery Key window follows MP creation (§5.4).
    let panels = fx.events_named("secure_panel_visible");
    assert_eq!(panels.len(), 4);
    assert_eq!(panels[0]["visible"], true);
    assert_eq!(panels[0]["title"], "Source Vault — Create Master Password");
    assert_eq!(panels[1]["visible"], false);
    assert_eq!(panels[2]["title"], "Source Vault — Recovery Key");
    assert_eq!(panels[3]["visible"], false);

    // unlock → UNLOCKED.
    let resp = unlock(&fx, MP);
    assert_eq!(resp["ok"], true, "{resp}");
    assert_eq!(fx.state(), VaultState::Unlocked);

    // add → metadata-only list (§13.2: no secret fields cross IPC).
    let r = add_login(&fx);
    let resp = fx.op(json!({"op": "list_items"}));
    assert_eq!(resp["ok"], true, "{resp}");
    let items = resp["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["ref"], r.as_str());
    assert_eq!(items[0]["title"], "Fixture Login");
    assert!(
        !resp.to_string().contains(PASSWORD),
        "secret leaked into list_items: {resp}"
    );

    // reveal (§14.4: capture check then presence, one record's secret).
    let la_before = fx.la.calls.load(Ordering::SeqCst);
    let resp = fx.op(json!({"op": "reveal", "ref": r}));
    assert_eq!(resp["ok"], true, "{resp}");
    assert_eq!(resp["secret"]["password"], PASSWORD);
    assert_eq!(fx.capture.calls.load(Ordering::SeqCst), 1);
    assert_eq!(fx.la.calls.load(Ordering::SeqCst), la_before + 1);

    // update the password, reveal reflects it, old value in history.
    let resp = fx.op(json!({"op": "update_item", "ref": r, "password": PASSWORD2}));
    assert_eq!(resp["ok"], true, "{resp}");
    let resp = fx.op(json!({"op": "reveal", "ref": r}));
    assert_eq!(resp["secret"]["password"], PASSWORD2, "{resp}");

    // delete → gone from list, reveal hits NOT_FOUND.
    let resp = fx.op(json!({"op": "delete_item", "ref": r}));
    assert_eq!(resp["ok"], true, "{resp}");
    let resp = fx.op(json!({"op": "list_items"}));
    assert_eq!(resp["items"].as_array().unwrap().len(), 0);
    let resp = fx.op(json!({"op": "reveal", "ref": r}));
    assert_eq!(err_code(&resp), "NOT_FOUND");

    // explicit lock zeroizes and returns to LOCKED.
    let events = fx
        .core
        .lock()
        .unwrap()
        .lock(LockReason::Explicit);
    assert!(events.iter().any(|e| e["event"] == "locked"));
    assert_eq!(fx.state(), VaultState::Locked);
    std::fs::remove_dir_all(&fx.dir).ok();
}

// --- OP-02: wrong MP → WRONG_CREDENTIAL, backoff, retry succeeds -------------

#[test]
fn op02_wrong_master_password_backoff_then_success() {
    let _g = serial();
    let fx = fx();
    assert_eq!(setup_vault(&fx, MP)["ok"], true);

    fx.push_panel(submitted(b"definitely-the-wrong-password"));
    let started = std::time::Instant::now();
    let resp = fx.op(json!({"op": "begin_recovery_unlock", "kind": "mp"}));
    assert_eq!(err_code(&resp), "WRONG_CREDENTIAL", "{resp}");
    assert_eq!(fx.state(), VaultState::Locked);
    // §15: first backoff step is 500 ms.
    assert!(started.elapsed() >= Duration::from_millis(500));
    assert_eq!(fx.core.lock().unwrap().failed_attempts, 1);

    // Correct MP still works and resets the counter.
    let resp = unlock(&fx, MP);
    assert_eq!(resp["ok"], true, "{resp}");
    assert_eq!(fx.core.lock().unwrap().failed_attempts, 0);
    std::fs::remove_dir_all(&fx.dir).ok();
}

// --- OP-03: presence denial blocks mutation, state restored ------------------

#[test]
fn op03_presence_denied_blocks_mutation() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);

    // Deny presence for the mutation.
    let denied = Arc::new(La {
        allow: false,
        calls: AtomicUsize::new(0),
    });
    let mut fx = fx;
    fx.set_presence(denied);
    let resp = fx.op(json!({
        "op": "add_item",
        "kind": "login",
        "title": "Denied",
        "password": PASSWORD,
        "hosts": ["example.test"],
    }));
    assert_eq!(err_code(&resp), "PRESENCE_DENIED", "{resp}");
    // State returns to UNLOCKED (§13.3: denial is not a lock event).
    assert_eq!(fx.state(), VaultState::Unlocked);
    // The event log shows AUTHORIZING entered and left.
    let states = fx.events_named("state");
    let names: Vec<&str> = states
        .iter()
        .filter_map(|e| e["state"].as_str())
        .collect();
    assert!(names.contains(&"authorizing"), "{names:?}");
    assert_eq!(names.last(), Some(&"unlocked"), "{names:?}");
    // Nothing was written.
    let resp = fx.op(json!({"op": "list_items"}));
    assert_eq!(resp["items"].as_array().unwrap().len(), 0);
    std::fs::remove_dir_all(&fx.dir).ok();
}

// --- OP-06: reveal refuses when capture suppression is unverifiable ----------

#[test]
fn op06_reveal_capture_unsafe_is_fail_closed() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let r = add_login(&fx);
    let la_calls = fx.la.calls.load(Ordering::SeqCst);

    let unsafe_capture = Arc::new(Capture {
        suppressed: false,
        calls: AtomicUsize::new(0),
    });
    let mut fx = fx;
    fx.set_capture(unsafe_capture);
    let resp = fx.op(json!({"op": "reveal", "ref": r}));
    assert_eq!(err_code(&resp), "CAPTURE_UNSAFE", "{resp}");
    // §14.4: the refusal happens BEFORE the presence prompt.
    assert_eq!(fx.la.calls.load(Ordering::SeqCst), la_calls);
    // §14.4 event emitted; no secret anywhere in the response.
    assert_eq!(fx.events_named("capture_unsafe").len(), 1);
    assert!(!resp.to_string().contains(PASSWORD));
    assert_eq!(fx.state(), VaultState::Unlocked);
    std::fs::remove_dir_all(&fx.dir).ok();
}

// --- OP-05: panel cancel paths ------------------------------------------------

#[test]
fn op05_cancelled_panels_leave_no_partial_state() {
    let _g = serial();
    // setup cancel: no vault files, still UNINITIALIZED.
    let fx = fx(); // empty panel queue → Cancelled
    let resp = fx.op(json!({"op": "setup_vault"}));
    assert_eq!(err_code(&resp), "PANEL_CANCELLED", "{resp}");
    assert_eq!(fx.state(), VaultState::Uninitialized);
    assert!(!fx.dir.join(VAULT_HEADER_NAME).exists());
    std::fs::remove_dir_all(&fx.dir).ok();

    // unlock cancel: back to LOCKED, retryable.
    let fx = vault_fx::fx(); // the local `fx` shadows the fn
    assert_eq!(setup_vault(&fx, MP)["ok"], true);
    let resp = fx.op(json!({"op": "begin_recovery_unlock", "kind": "mp"}));
    assert_eq!(err_code(&resp), "PANEL_CANCELLED", "{resp}");
    assert_eq!(fx.state(), VaultState::Locked);
    assert_eq!(unlock(&fx, MP)["ok"], true);
    std::fs::remove_dir_all(&fx.dir).ok();
}

// --- OP-07: change_master_password re-wraps, old MP dies ----------------------

#[test]
fn op07_change_master_password_rewraps_vk() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let r = add_login(&fx);

    fx.push_panel(PanelOutcome::SubmittedChange(
        SecretVec::new(MP.to_vec()),
        SecretVec::new(MP_NEW.to_vec()),
    ));
    let resp = fx.op(json!({"op": "change_master_password"}));
    assert_eq!(resp["ok"], true, "{resp}");
    // Still UNLOCKED — the op re-seals in place.
    assert_eq!(fx.state(), VaultState::Unlocked);

    fx.core.lock().unwrap().lock(LockReason::Explicit);
    // Old MP is dead.
    fx.push_panel(submitted(MP));
    let resp = fx.op(json!({"op": "begin_recovery_unlock", "kind": "mp"}));
    assert_eq!(err_code(&resp), "WRONG_CREDENTIAL", "{resp}");
    // New MP opens the vault and the data survived the re-wrap.
    let resp = unlock(&fx, MP_NEW);
    assert_eq!(resp["ok"], true, "{resp}");
    let resp = fx.op(json!({"op": "reveal", "ref": r}));
    assert_eq!(resp["secret"]["password"], PASSWORD, "{resp}");
    std::fs::remove_dir_all(&fx.dir).ok();
}
