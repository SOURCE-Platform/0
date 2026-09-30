//! §22.4 (F2-D3): retained history, restore, and the bulk-deletion gate
//! (AU-06, AU-07). Stub panels; synthetic records only.

mod vault_fx;

use serde_json::json;
use vault_fx::*;

fn add(fx: &Fx, title: &str) -> String {
    let r = fx.op(json!({"op": "add_item", "kind": "login", "title": title, "username": "u@example.test", "hosts": ["example.test"], "password": PASSWORD}));
    assert_eq!(r["ok"], true, "{r}");
    r["ref"].as_str().unwrap().to_string()
}

/// AU-07: a deleted record comes back as a NEW record (the tombstone
/// stands); on a live record a restore is an ordinary new revision.
#[test]
fn restore_after_deletion_and_on_a_live_record() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let r = add(&fx, "first title");
    let upd = fx.op(json!({"op": "update_item", "ref": r, "title": "second title"}));
    assert_eq!(upd["ok"], true, "{upd}");
    let hist = fx.op(json!({"op": "list_history", "ref": r}));
    let revs = hist["revisions"].as_array().unwrap().clone();
    assert_eq!(revs.len(), 2, "{hist}");
    assert!(!hist.to_string().contains(PASSWORD), "history is metadata only");
    let first = revs.iter().find(|v| v["current"] == false).unwrap()["revision_id"].as_str().unwrap().to_string();

    // Live record: the old content returns as a successor revision.
    let back = fx.op(json!({"op": "restore_revision", "ref": r, "revision_id": first}));
    assert_eq!(back["ref"], r.as_str(), "{back}");
    let items = fx.op(json!({"op": "list_items"}));
    assert_eq!(items["items"][0]["title"], "first title");
    assert_eq!(fx.op(json!({"op": "list_history", "ref": r}))["revisions"].as_array().unwrap().len(), 3);

    // Deleted record: listed, and restored as a new record.
    assert_eq!(fx.op(json!({"op": "delete_item", "ref": r}))["ok"], true);
    let gone = fx.op(json!({"op": "list_deleted"}));
    assert_eq!(gone["items"][0]["ref"], r.as_str(), "{gone}");
    assert_eq!(gone["items"][0]["title"], "first title");
    let rev = gone["items"][0]["revision_id"].as_str().unwrap().to_string();
    let back = fx.op(json!({"op": "restore_revision", "ref": r, "revision_id": rev}));
    assert_eq!(back["ok"], true, "{back}");
    let new_ref = back["ref"].as_str().unwrap().to_string();
    assert_ne!(new_ref, r, "never a resurrection of the tombstoned record");
    let items = fx.op(json!({"op": "list_items"}));
    assert_eq!(items["items"].as_array().unwrap().len(), 1);
    assert_eq!(items["items"][0]["ref"], new_ref.as_str());
    assert_eq!(fx.op(json!({"op": "reveal", "ref": new_ref}))["secret"]["password"], PASSWORD);
    assert_eq!(fx.op(json!({"op": "list_deleted"}))["items"].as_array().unwrap().len(), 1, "the tombstone stands");

    // A tombstone itself, or another record's revision, cannot be restored.
    let tomb = fx.op(json!({"op": "list_history", "ref": r}))["revisions"].as_array().unwrap().iter().find(|v| v["deleted"] == true).unwrap()["revision_id"].as_str().unwrap().to_string();
    assert_eq!(err_code(&fx.op(json!({"op": "restore_revision", "ref": r, "revision_id": tomb}))), "NOT_FOUND");
    assert_eq!(err_code(&fx.op(json!({"op": "restore_revision", "ref": new_ref, "revision_id": rev}))), "NOT_FOUND");
    fx.remove_dir();
}

/// AU-06: ten deletions pass on presence; the eleventh inside the window
/// needs the master password, and the count survives lock and reboot.
#[test]
fn bulk_deletion_needs_the_master_password() {
    let _g = serial();
    let mut fx = fx();
    setup_and_unlock(&fx);
    let refs: Vec<String> = (0..12).map(|i| add(&fx, &format!("item {i}"))).collect();
    for r in &refs[..10] {
        assert_eq!(fx.op(json!({"op": "delete_item", "ref": r}))["ok"], true);
    }
    // No MP typed.
    assert_eq!(err_code(&fx.op(json!({"op": "delete_item", "ref": refs[10]}))), "PANEL_CANCELLED");
    fx.push_panel(submitted(b"not the master password"));
    assert_eq!(err_code(&fx.op(json!({"op": "delete_item", "ref": refs[10]}))), "WRONG_CREDENTIAL");
    // Lock + restart do not reset the count.
    fx.reboot();
    assert_eq!(unlock(&fx, MP)["ok"], true);
    assert_eq!(err_code(&fx.op(json!({"op": "delete_item", "ref": refs[10]}))), "PANEL_CANCELLED");
    assert_eq!(fx.op(json!({"op": "list_items"}))["items"].as_array().unwrap().len(), 2, "nothing deleted without the MP");
    fx.push_panel(submitted(MP));
    assert_eq!(fx.op(json!({"op": "delete_item", "ref": refs[10]}))["ok"], true);
    assert_eq!(fx.state(), VaultState::Unlocked);
    fx.remove_dir();
}
