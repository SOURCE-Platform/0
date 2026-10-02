//! Spec v0.5 §22.7 on the Mac (PS-08, PS-10): a revocation refuses for
//! good whatever only the revoked phone delivered; a rotation never
//! carries a peer-only revision into the new key, and this Mac's own edit
//! built on one is re-authored with its content, so a later index stays
//! ancestor-closed. Synthetic data only.

mod device_fx;
mod peer_fx;
mod vault_fx;

use peer_fx::*;
use vault_fx::*;
use vault_helper::storage::revisions::get_row;
use vault_helper::storage::VaultStore;
use vault_proto::peer::PeerOp;

const PT: &[u8] = br#"{"title":"phone item","username":"p@example.test","password":"synthetic","urls":[{"host":"example.test","match":"exact","allow_http":false}]}"#;
const META: &[u8] = br#"{"title":"phone item","username":"p@example.test","hosts":["example.test"]}"#;

/// The phone delivers one new record over the peer path.
fn phone_delivers(w: &W) -> (String, [u8; 32]) {
    let c = ctx(w, true, false, false);
    let (dir, mut ps, vk) = phone_store(w);
    let rid = ps.add_record(&vk, 1, PT, META).unwrap();
    let row = row_of(&ps, &rid);
    assert_eq!(ask(w, &c, PeerOp::RevsPut, put_body(&[row.clone()])).0, vault_proto::peer::PeerStatus::Ok);
    drop(ps);
    let _ = std::fs::remove_dir_all(dir);
    (rid, row.revision_id)
}

/// PS-08: the revoker refuses what only the revoked phone delivered.
#[test]
fn revoking_a_phone_refuses_what_only_it_delivered() {
    let _g = serial();
    let w = world("cut-revoke");
    let (_, rev) = phone_delivers(&w);
    assert!(w.fx.op(serde_json::json!({"op": "list_items"}))["items"].to_string().contains("phone item"));
    w.fx.push_panel(submitted(MP));
    let r = w.fx.op(serde_json::json!({"op": "revoke_device", "device_id": vault_helper::crypto::hex::encode(w.id)}));
    assert_eq!(r["ok"], true, "{r}");
    let store = VaultStore::open(&w.fx.dir).unwrap();
    assert!(get_row(&store.conn, &rev).unwrap().is_none(), "gone from the graph");
    assert!(vault_helper::storage::set_aside::refused(&store.conn, &rev).unwrap(), "refused for good");
    assert!(!w.fx.op(serde_json::json!({"op": "list_items"}))["items"].to_string().contains("phone item"));
    w.fx.remove_dir();
}

/// PS-10: an own edit on top of a peer-only revision survives a rotation,
/// re-authored; the peer-only revision is set aside, not refused.
#[test]
fn a_rotation_sets_aside_peer_only_revisions_and_keeps_own_edits() {
    let _g = serial();
    let w = world("cut-rotate");
    let (rid, rev) = phone_delivers(&w);
    let edit = w.fx.op(serde_json::json!({"op": "update_item", "ref": rid, "title": "edited on the Mac"}));
    assert_eq!(edit["ok"], true, "{edit}");
    w.fx.push_panel(submitted(MP));
    assert_eq!(w.fx.op(serde_json::json!({"op": "rotate_recovery_key"}))["ok"], true);
    let store = VaultStore::open(&w.fx.dir).unwrap();
    assert!(get_row(&store.conn, &rev).unwrap().is_none(), "set aside");
    assert!(!vault_helper::storage::set_aside::refused(&store.conn, &rev).unwrap(), "only set aside, not refused");
    let items = w.fx.op(serde_json::json!({"op": "list_items"}));
    assert!(items["items"].to_string().contains("edited on the Mac"), "{items}");
    // Every remaining revision's parents are present: an index would be
    // ancestor-closed.
    for r in vault_helper::storage::revision_rows::all_rows(&store.conn).unwrap() {
        for p in &r.parent_ids {
            assert!(get_row(&store.conn, p).unwrap().is_some(), "dangling parent");
        }
    }
    w.fx.remove_dir();
}
