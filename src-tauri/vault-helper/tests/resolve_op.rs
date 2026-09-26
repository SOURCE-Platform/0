//! `resolve_conflict` at the IPC layer (VER-I5, SEC-O4): a frozen record
//! without `acknowledge_tamper` is refused before any presence prompt;
//! with it, the prompt runs and the freeze clears. Synthetic data only.

mod vault_fx;

use std::sync::atomic::Ordering;

use serde_json::json;
use vault_fx::*;

#[test]
fn frozen_record_needs_acknowledgement_before_the_prompt() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let r = add_login(&fx);
    // Freeze the record as sync would on equivocation evidence.
    let head: Vec<u8> = {
        let conn = rusqlite::Connection::open(fx.dir.join("vault.db")).unwrap();
        let head: Vec<u8> = conn.query_row("SELECT tip_rev FROM record_tips WHERE record_id=?1", [&r], |x| x.get(0)).unwrap();
        conn.execute("INSERT INTO record_flags (record_id, frozen, evidence) VALUES (?1, 1, ?2)", rusqlite::params![r, head]).unwrap();
        head
    };
    let chosen = vault_helper::crypto::hex::encode(&head);
    let before = fx.la.calls.load(Ordering::SeqCst);
    let resp = fx.op(json!({"op": "resolve_conflict", "ref": r, "chosen_rev": chosen}));
    assert_eq!(err_code(&resp), "CONFLICT_PENDING", "{resp}");
    assert_eq!(fx.la.calls.load(Ordering::SeqCst), before, "no prompt without the acknowledgement");
    let resp = fx.op(json!({"op": "resolve_conflict", "ref": r, "chosen_rev": chosen, "acknowledge_tamper": true}));
    assert_eq!(resp["ok"], true, "{resp}");
    assert_eq!(fx.la.calls.load(Ordering::SeqCst), before + 1);
    let items = fx.op(json!({"op": "list_items"}));
    let item = items["items"].as_array().unwrap().iter().find(|i| i["ref"] == r.as_str()).unwrap().clone();
    assert!(item.get("tamper").is_none() && item.get("conflicted").is_none(), "{item}");
    fx.remove_dir();
}
