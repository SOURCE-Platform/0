//! Fail-closed storage paths surfacing through ops: FORMAT_TOO_NEW,
//! MANIFEST_MISMATCH, RECORD_CORRUPT (§3.5/§3.6) and §2.8 rollback
//! evidence. Each must enter ERROR (or quarantine the record) and never
//! leak or delete anything.

mod vault_fx;

use vault_fx::*;

// --- Fail-closed storage surfacing through ops --------------------------------

#[test]
fn format_too_new_header_enters_error() {
    let _g = serial();
    let fx = fx();
    assert_eq!(setup_vault(&fx, MP)["ok"], true);
    // A future-format header: boot caches FORMAT_TOO_NEW; the unlock path
    // surfaces it and enters ERROR (§3.6), never touching the wrap.
    let header_path = fx.dir.join(VAULT_HEADER_NAME);
    let mut header: Value =
        serde_json::from_slice(&std::fs::read(&header_path).unwrap()).unwrap();
    header["version"] = json!(99);
    std::fs::write(&header_path, header.to_string()).unwrap();

    let mut fx = fx;
    fx.reboot();
    let resp = unlock(&fx, MP);
    assert_eq!(err_code(&resp), "FORMAT_TOO_NEW", "{resp}");
    assert_eq!(fx.state(), VaultState::Error);
    // And no panel was ever presented.
    assert_eq!(fx.panel.seen.lock().unwrap().len(), 1, "setup panel only");
    fx.remove_dir();
}

#[test]
fn manifest_mismatch_enters_error() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let r = add_login(&fx);
    fx.core.lock().unwrap().lock(LockReason::Explicit);

    // Swap the manifest's revision id for a made-up one: the manifest no
    // longer describes the DB (§3.5) → MANIFEST_MISMATCH, ERROR state.
    let manifest_path = fx.dir.join(MANIFEST_NAME);
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["objects"][0]["revision_id"] = json!("00".repeat(32));
    std::fs::write(&manifest_path, manifest.to_string()).unwrap();

    let resp = unlock(&fx, MP);
    assert_eq!(err_code(&resp), "MANIFEST_MISMATCH", "{resp}");
    assert_eq!(fx.state(), VaultState::Error);
    // The helper never deletes the file (§3.6) — evidence on disk stays.
    assert!(manifest_path.exists());
    let _ = r;
    fx.remove_dir();
}

#[test]
fn record_corruption_fails_closed_at_read() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let r = add_login(&fx);

    // Corrupt the tip revision's ciphertext in place (valid manifest,
    // poisoned record). AEAD must fail → RECORD_CORRUPT, no partial read.
    {
        let store_dir = fx.dir.clone();
        let conn = rusqlite::Connection::open(store_dir.join("vault.db")).unwrap();
        conn.execute(
            "UPDATE record_revs SET ct = randomblob(length(ct))
             WHERE revision_id = (SELECT tip_rev FROM record_tips WHERE record_id=?1)",
            [r.as_str()],
        )
        .unwrap();
    }

    let resp = fx.op(json!({"op": "reveal", "ref": r}));
    assert_eq!(err_code(&resp), "RECORD_CORRUPT", "{resp}");
    assert!(!resp.to_string().contains(PASSWORD));
    // Other ops on the poisoned record fail the same way, but the vault
    // itself stays usable (the corruption is quarantined to the record).
    let resp = fx.op(json!({"op": "update_item", "ref": r, "title": "x"}));
    assert_eq!(err_code(&resp), "RECORD_CORRUPT", "{resp}");
    assert_eq!(fx.state(), VaultState::Unlocked);
    let r2 = add_login(&fx);
    let resp = fx.op(json!({"op": "reveal", "ref": r2}));
    assert_eq!(resp["secret"]["password"], PASSWORD, "{resp}");
    fx.remove_dir();
}

// --- §2.8 rollback evidence -----------------------------------------------------

#[test]
fn rollback_evidence_refuses_older_generation() {
    let _g = serial();
    let fx = fx();
    assert_eq!(setup_vault(&fx, MP)["ok"], true); // fresh vault: generation 1
    // The keychain remembers a NEWER generation than this directory holds
    // (§2.8: a stale/rolled-back copy presented later) → fatal refusal.
    keychain::write_seen_generation(2).unwrap();
    let resp = unlock(&fx, MP);
    assert_eq!(err_code(&resp), "MANIFEST_ROLLBACK", "{resp}");
    assert_eq!(fx.state(), VaultState::Error);
    fx.remove_dir();
}
