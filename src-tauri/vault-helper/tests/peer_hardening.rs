//! F.2c review fixes on the Mac (phase-f2-verification §5): the set-aside
//! is one transaction and one flip (SEC-B1 / VER-B4) and keeps this Mac's
//! deletions (SEC-I3); a forged author never makes a peer's edit "own"
//! (SEC-I2); the exchange cap (SEC-I1 / PW-09); the wire answers of annex
//! A.1/A.3 (VER-I3); who may speak with a pending revocation (VER-I8);
//! the LOCKED inbox bounds and purge (PS-11). Synthetic data only.

mod device_fx;
mod peer_fx;
mod vault_fx;

use std::collections::HashSet;

use peer_fx::*;
use serde_json::json;
use vault_fx::*;
use vault_helper::peer::{inbox, verify, Refusal};
use vault_helper::registry::device::DeviceIdentity;
use vault_helper::storage::revisions::{get_row, heads};
use vault_helper::storage::set_aside;
use vault_helper::storage::VaultStore;
use vault_proto::peer::body;
use vault_proto::peer::exchange::{encode_revs_get, Revs};
use vault_proto::peer::{PeerOp, PeerRequest, PeerStatus};

const PT: &[u8] = br#"{"title":"phone item","username":"p@example.test","password":"synthetic","urls":[{"host":"example.test","match":"exact","allow_http":false}]}"#;
const META: &[u8] = br#"{"title":"phone item","username":"p@example.test","hosts":["example.test"]}"#;

fn vk(w: &W) -> vault_helper::crypto::secret::SecretBytes<32> {
    w.fx.core.lock().unwrap().vk.as_ref().map(|v| vault_helper::crypto::secret::SecretBytes::new(*v.expose())).unwrap()
}

/// The phone adds one record over the peer path; returns (record, revision).
fn phone_adds(w: &W) -> (String, [u8; 32]) {
    let c = ctx(w, true, false, false);
    let (dir, mut ps, k) = phone_store(w);
    let rid = ps.add_record(&k, 1, PT, META).unwrap();
    let row = row_of(&ps, &rid);
    assert_eq!(ask(w, &c, PeerOp::RevsPut, put_body(&[row.clone()])).0, PeerStatus::Ok);
    drop(ps);
    let _ = std::fs::remove_dir_all(dir);
    (rid, row.revision_id)
}

/// SEC-B1: the set-aside leaves a vault that opens on its own — before,
/// its deletions were outside any flip, and a rotation failing afterwards
/// left `manifest.json` listing revisions the DB no longer had.
#[test]
fn a_set_aside_on_its_own_leaves_an_openable_vault() {
    let _g = serial();
    let w = world("hard-flip");
    let (rid, rev) = phone_adds(&w);
    let edit = w.fx.op(json!({"op": "update_item", "ref": rid, "title": "edited on the Mac"}));
    assert_eq!(edit["ok"], true, "{edit}");
    {
        // No rotation follows: as if it failed right after the set-aside.
        let mut store = VaultStore::open(&w.fx.dir).unwrap();
        assert_eq!(set_aside::set_aside(&mut store, &vk(&w)).unwrap(), 1, "the phone's revision, without a copy");
    }
    let store = VaultStore::open(&w.fx.dir).expect("opens: the set-aside flipped the manifest");
    assert!(get_row(&store.conn, &rev).unwrap().is_none());
    let h = heads(&store.conn, &rid).unwrap();
    assert_eq!(h.len(), 1);
    let head = get_row(&store.conn, &h[0]).unwrap().unwrap();
    assert!(head.parent_ids.is_empty(), "re-authored onto its nearest remaining ancestor: none");
    w.fx.remove_dir();
}

/// SEC-I3: the Mac deleting a phone-delivered record stays deleted across
/// a rotation — the tombstone is re-authored, not dropped.
#[test]
fn a_rotation_keeps_this_macs_deletions() {
    let _g = serial();
    let w = world("hard-tomb");
    let (rid, _) = phone_adds(&w);
    let del = w.fx.op(json!({"op": "delete_item", "ref": rid}));
    assert_eq!(del["ok"], true, "{del}");
    w.fx.push_panel(submitted(MP));
    assert_eq!(w.fx.op(json!({"op": "rotate_recovery_key"}))["ok"], true);
    assert!(!w.fx.op(json!({"op": "list_items"}))["items"].to_string().contains("phone item"));
    let store = VaultStore::open(&w.fx.dir).unwrap();
    let h = heads(&store.conn, &rid).unwrap();
    assert!(get_row(&store.conn, &h[0]).unwrap().unwrap().deleted, "a re-authored tombstone heads the record");
    w.fx.remove_dir();
}

/// SEC-I2: a revision a peer delivered claiming this Mac as its author is
/// still the peer's — never re-authored as the Mac's own when the peer's
/// revisions are refused.
#[test]
fn a_forged_mac_author_is_not_an_own_revision() {
    let _g = serial();
    let w = world("hard-forge");
    let c = ctx(&w, true, false, false);
    let (dir, mut ps, k) = phone_store(&w);
    let rid = ps.add_record(&k, 1, PT, META).unwrap();
    let first = row_of(&ps, &rid);
    let mac = c.me.device_id();
    ps.set_author_device(&mac).unwrap();
    ps.write_successor(&k, &rid, 1, first.schema_version, PT, META, first.created_at).unwrap();
    let forged = row_of(&ps, &rid);
    assert_eq!(forged.author_device, vault_helper::storage::revisions::uuid_string(&mac));
    assert_eq!(ask(&w, &c, PeerOp::RevsPut, put_body(&[first.clone(), forged.clone()])).0, PeerStatus::Ok);
    drop(ps);
    let _ = std::fs::remove_dir_all(dir);
    let store = VaultStore::open(&w.fx.dir).unwrap();
    let me = vault_helper::storage::revisions::uuid_string(&mac);
    let mine = vault_helper::storage::revoked::record(&store.conn, &w.id, &HashSet::new(), &me).unwrap();
    assert!(mine.iter().all(|r| r.revision_id != forged.revision_id), "not re-authored as the Mac's");
    assert!(get_row(&store.conn, &forged.revision_id).unwrap().is_none(), "refused with the phone's edit");
    w.fx.remove_dir();
}

/// PW-09 (SEC-I1): 64 MiB of answer bodies per sender in 10 minutes.
#[test]
fn the_exchange_cap_stops_at_64_mib_per_sender() {
    verify::reset_rate();
    let (a, b) = ([0xA1; 16], [0xB2; 16]);
    assert!(verify::spend(&a, verify::EXCHANGE_BYTES - 10));
    assert!(verify::spend(&a, 10));
    assert!(!verify::spend(&a, 1), "the cap is reached");
    assert!(verify::spend(&b, 1), "another sender has its own");
    verify::reset_rate();
}

/// Annex A.1/A.3 (VER-I3): an unknown operation and a body on a bodiless
/// one are status 4 after authentication; a put over 2,000 revisions is
/// status 2; a record not held is listed as unavailable (reason 2).
#[test]
fn malformed_and_oversized_requests_get_their_signed_status() {
    let _g = serial();
    let w = world("hard-wire");
    let c = ctx(&w, true, false, false);
    assert_eq!(ask(&w, &c, PeerOp::Unknown(9), body::empty()).0, PeerStatus::FormatInvalid);
    assert_eq!(ask(&w, &c, PeerOp::Hello, vec![0, 0, 0, 0, 1, 0xFE]).0, PeerStatus::FormatInvalid);
    let over = Revs { complete: None, objects: vec![vec![1]; 2_001], unavailable: vec![] }.encode();
    assert_eq!(ask(&w, &c, PeerOp::RevsPut, over).0, PeerStatus::Limit);
    let (st, b) = ask(&w, &c, PeerOp::RevsGet, encode_revs_get(&[([0x42; 16], vec![])]));
    assert_eq!(st, PeerStatus::Ok);
    assert_eq!(Revs::decode(&b, false).unwrap().unavailable, vec![([0x42; 16], body::NOT_HELD)]);
    w.fx.remove_dir();
}

/// §22.8 who may speak (VER-I8): the target of a pending revocation is
/// refused everything but `peer_status`.
#[test]
fn a_pending_revocation_target_may_ask_only_for_status() {
    let _g = serial();
    let w = world("hard-speak");
    let c = ctx(&w, true, false, false);
    let store = VaultStore::open(&w.fx.dir).unwrap();
    let base = vault_helper::sync::pending::Base::of(&store.header);
    vault_helper::sync::pending::add(&store.conn, vault_helper::sync::pending::PendingOp::Revocation, true, base, vec![], NOW).unwrap();
    vault_helper::sync::pending::note_target(&store.conn, &w.id).unwrap();
    let req = |op| PeerRequest { vault_id: c.vault_id, sender_device_id: w.id, receiver_device_id: c.me.device_id(), operation: op, body_sha256: [0; 32], t: NOW, n: [9; 16] };
    assert_eq!(verify::may_speak(&c, &store, &req(PeerOp::Hello)).err(), Some(Refusal::Forbidden));
    assert!(verify::may_speak(&c, &store, &req(PeerOp::Status)).is_ok());
    w.fx.remove_dir();
}

/// PS-11: the LOCKED inbox refuses past its bounds, refuses a malformed
/// batch, and a cutoff purges what the peer left there.
#[test]
fn the_locked_inbox_is_bounded_validated_and_purged() {
    let _g = serial();
    let w = world("hard-inbox");
    let (dir, mut ps, k) = phone_store(&w);
    let rid = ps.add_record(&k, 1, PT, META).unwrap();
    let row = row_of(&ps, &rid);
    drop(ps);
    let _ = std::fs::remove_dir_all(dir);
    let store = VaultStore::open(&w.fx.dir).unwrap();
    assert_eq!(inbox::stash(&store, &w.id, &put_body(&[row.clone()]), NOW), Ok(1));
    let over = Revs { complete: None, objects: vec![vec![1]; 2_001], unavailable: vec![] }.encode();
    assert_eq!(inbox::stash(&store, &w.id, &over, NOW), Err(vault_helper::errors::ErrorCode::PeerLimit));
    let garbage = Revs { complete: None, objects: vec![vec![1, 2, 3]], unavailable: vec![] }.encode();
    assert_eq!(inbox::stash(&store, &w.id, &garbage, NOW), Err(vault_helper::errors::ErrorCode::FormatInvalid));
    set_aside::cut_off(&store.conn, &w.id, &HashSet::new()).unwrap();
    let held: i64 = store.conn.query_row("SELECT count(*) FROM peer_inbox", [], |r| r.get(0)).unwrap();
    assert_eq!(held, 0, "purged with the cutoff");
    w.fx.remove_dir();
}

fn node(id: u8, parents: &[u8], deleted: bool) -> vault_helper::storage::revisions::RevisionRow {
    vault_helper::storage::revisions::RevisionRow {
        revision_id: [id; 32],
        record_id: "00000000-0000-4000-8000-000000000001".into(),
        parent_ids: parents.iter().map(|p| [*p; 32]).collect(),
        author_device: "00000000-0000-4000-8000-000000000002".into(),
        counter: u64::from(id),
        deleted,
        kind_tag: 1,
        vk_generation: 1,
        schema_version: 1,
        nonce: [0; 24],
        ct: vec![],
        meta_nonce: [0; 24],
        meta_ct: vec![],
        created_at: 0,
        updated_at: 0,
    }
}

/// Annex A.3.4 (VER-I8 / VER-I11): the canonical order is Kahn's
/// algorithm emitting the **smallest** ready id — pinned on a diamond whose
/// two middle revisions are ready together; and served heads follow the
/// store's tombstone rule (VER-O2).
#[test]
fn the_canonical_order_and_served_heads_are_pinned() {
    use vault_helper::peer::graph::{closure, heads};
    let rows = vec![node(0x50, &[], false), node(0x90, &[0x50], false), node(0x30, &[0x50], false), node(0x70, &[0x30, 0x90], false)];
    let order: Vec<u8> = closure(&rows, &[]).iter().map(|r| r.revision_id[0]).collect();
    assert_eq!(order, vec![0x50, 0x30, 0x90, 0x70]);
    // What the requester holds is left out.
    let order: Vec<u8> = closure(&rows, &[[0x30; 32]]).iter().map(|r| r.revision_id[0]).collect();
    assert_eq!(order, vec![0x90, 0x70]);
    // A tombstone under a single-parent edit stays a head; under a
    // two-parent resolution it does not.
    let edit = vec![node(0x10, &[], true), node(0x20, &[0x10], false)];
    assert_eq!(heads(&edit), vec![[0x10; 32], [0x20; 32]]);
    let resolved = vec![node(0x10, &[], true), node(0x11, &[], false), node(0x20, &[0x10, 0x11], false)];
    assert_eq!(heads(&resolved), vec![[0x20; 32]]);
}
