//! F.2c re-review follow-ups (phase-f2-verification §5.1): the set-aside
//! with no own descendant (VER-I12) and on a many-path peer graph
//! (SEC-B3 / VER-B5); a peer never leaves waiting rows (VER-I14); the
//! exchange cap through the serving path (VER-I13); freshness ends at lock
//! and peer sessions are capped (VER-I14); a Mac without Touch ID unlocks
//! with the master password only (owner decision 2026-10-02). Synthetic
//! data only.

mod device_fx;
mod peer_fx;
mod vault_fx;

use std::time::Instant;

use peer_fx::*;
use serde_json::json;
use vault_fx::*;
use vault_helper::peer::verify;
use vault_helper::storage::revisions::{get_row, heads, insert_rev, RevisionRow};
use vault_helper::storage::sources::{self, Source};
use vault_helper::storage::{merge, rev_state, set_aside, VaultStore};
use vault_proto::peer::body::PutCounts;
use vault_proto::peer::{PeerOp, PeerStatus};

const PT: &[u8] = br#"{"title":"phone item","username":"p@example.test","password":"synthetic","urls":[{"host":"example.test","match":"exact","allow_http":false}]}"#;
const META: &[u8] = br#"{"title":"phone item","username":"p@example.test","hosts":["example.test"]}"#;

fn vk(w: &W) -> vault_helper::crypto::secret::SecretBytes<32> {
    w.fx.core.lock().unwrap().vk.as_ref().map(|v| vault_helper::crypto::secret::SecretBytes::new(*v.expose())).unwrap()
}

/// VER-I12: the phone's record alone (nothing of the Mac's on it) — before
/// the fix, the set-aside's deletions left `manifest.json` listing it and
/// the next open failed `MANIFEST_MISMATCH`.
#[test]
fn a_set_aside_with_nothing_to_reauthor_still_flips() {
    let _g = serial();
    let w = world("fu-flip");
    let c = ctx(&w, true, false, false);
    let (dir, mut ps, k) = phone_store(&w);
    let rid = ps.add_record(&k, 1, PT, META).unwrap();
    let row = row_of(&ps, &rid);
    assert_eq!(ask(&w, &c, PeerOp::RevsPut, put_body(&[row.clone()])).0, PeerStatus::Ok);
    drop(ps);
    let _ = std::fs::remove_dir_all(dir);
    {
        let mut store = VaultStore::open(&w.fx.dir).unwrap();
        assert_eq!(set_aside::set_aside(&mut store, &vk(&w)).unwrap(), 1);
    }
    let store = VaultStore::open(&w.fx.dir).expect("opens after a set-aside with no rotation behind it");
    assert!(get_row(&store.conn, &row.revision_id).unwrap().is_none());
    w.fx.remove_dir();
}

fn ladder_row(base: &RevisionRow, id: [u8; 32], parents: Vec<[u8; 32]>, author: &str, counter: u64) -> RevisionRow {
    RevisionRow { revision_id: id, parent_ids: parents, author_device: author.into(), counter, ..base.clone() }
}

/// SEC-B3 / VER-B5: 40 diamond levels (120 peer-only revisions, 2^40
/// paths) under one Mac edit. The set-aside finishes at once, and the
/// edit lands on the Mac's own revision below the ladder.
#[test]
fn a_set_aside_over_a_many_path_peer_graph_finishes() {
    let _g = serial();
    let w = world("fu-ladder");
    let item = add_login(&w.fx);
    let mut store = VaultStore::open(&w.fx.dir).unwrap();
    let base = get_row(&store.conn, &heads(&store.conn, &item).unwrap()[0]).unwrap().unwrap();
    let phone = vault_helper::storage::revisions::uuid_string(&w.id);
    let mut below = base.revision_id;
    let mut n = 0u8;
    let mut next = |tag: u8| {
        n = n.wrapping_add(1);
        let mut id = [tag; 32];
        id[31] = n;
        id[30] = n / 2;
        id
    };
    for level in 0..40u64 {
        let (a, b, m) = (next(0xA0), next(0xB0), next(0xC0));
        let mut ab = [a, b];
        ab.sort();
        for (id, parents, counter) in [(a, vec![below], 3 * level + 1), (b, vec![below], 3 * level + 2), (m, ab.to_vec(), 3 * level + 3)] {
            insert_rev(&store.conn, &ladder_row(&base, id, parents, &phone, counter)).unwrap();
            sources::add(&store.conn, &id, Source::Peer(w.id)).unwrap();
        }
        below = m;
    }
    merge::recompute_heads(&store.conn, &item).unwrap();
    // Seal-free stand-ins are fine: peer-only rows are never opened, and
    // the Mac's edit is authored as usual on the ladder's top.
    let k = vk(&w);
    store.write_successor(&k, &item, base.kind_tag, base.schema_version, PT, META, base.created_at).unwrap();
    let started = Instant::now();
    set_aside::set_aside(&mut store, &k).unwrap();
    assert!(started.elapsed().as_secs() < 10, "linear, not per path");
    let h = heads(&store.conn, &item).unwrap();
    assert_eq!(h.len(), 1);
    assert_eq!(get_row(&store.conn, &h[0]).unwrap().unwrap().parent_ids, vec![base.revision_id], "nearest remaining ancestor");
    drop(store);
    VaultStore::open(&w.fx.dir).unwrap();
    w.fx.remove_dir();
}

/// VER-I14 (VER-I1 second half): a revision resting on a waiting one
/// waits too, and nothing a peer sent is left in `pending_revs`.
#[test]
fn a_peer_leaves_nothing_waiting_here() {
    let _g = serial();
    let w = world("fu-wait");
    let c = ctx(&w, true, false, false);
    let (dir, mut ps, k) = phone_store(&w);
    let rid = ps.add_record(&k, 1, PT, META).unwrap();
    let mut first = row_of(&ps, &rid);
    ps.write_successor(&k, &rid, 1, first.schema_version, PT, META, first.created_at).unwrap();
    let second = row_of(&ps, &rid);
    drop(ps);
    let _ = std::fs::remove_dir_all(dir);
    first.vk_generation += 1; // another key generation: waits
    let (st, b) = ask(&w, &c, PeerOp::RevsPut, put_body(&[first, second]));
    assert_eq!(st, PeerStatus::Ok);
    assert_eq!(PutCounts::decode(&b).unwrap(), PutCounts { admitted: 0, waiting: 2, refused: 0 });
    let store = VaultStore::open(&w.fx.dir).unwrap();
    assert_eq!(rev_state::pending_count(&store.conn).unwrap(), 0);
    w.fx.remove_dir();
}

/// VER-I13: the cap is 64 MiB, and a sender at the cap gets the signed
/// status 2 from the serving path itself.
#[test]
fn the_serving_path_answers_status_2_at_the_exchange_cap() {
    let _g = serial();
    let w = world("fu-cap");
    let c = ctx(&w, true, false, false);
    assert_eq!(verify::EXCHANGE_BYTES, 64 << 20);
    assert_eq!(ask(&w, &c, PeerOp::Hello, vault_proto::peer::body::empty()).0, PeerStatus::Ok);
    verify::reset_rate();
    // A hello body (the 8 KiB heads digest) no longer fits.
    assert!(verify::spend(&w.id, verify::EXCHANGE_BYTES - 10));
    assert_eq!(ask(&w, &c, PeerOp::Hello, vault_proto::peer::body::empty()).0, PeerStatus::Limit);
    verify::reset_rate();
    w.fx.remove_dir();
}

/// VER-I7 / VER-I14: freshness never survives a lock.
#[test]
fn freshness_ends_at_lock() {
    let _g = serial();
    let w = world("fu-fresh");
    w.fx.core.lock().unwrap().provider_checked = Some(vault_helper::vault::peer_serve::Checked::now());
    let _ = w.fx.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    assert!(w.fx.core.lock().unwrap().provider_checked.is_none());
    w.fx.remove_dir();
}

/// Owner decision 2026-10-02: a Mac that cannot make a Touch-ID-bound key
/// gets an agreement key nobody holds — no plain key — and unlocks with
/// the master password, with no presence prompt first.
#[test]
fn a_mac_without_touch_id_unlocks_with_the_master_password_only() {
    let _g = serial();
    std::env::set_var("OV0_VAULT_SE_BIOMETRY", "absent");
    let mut fx = fx();
    setup_and_unlock(&fx);
    std::env::remove_var("OV0_VAULT_SE_BIOMETRY");
    let me = vault_helper::device::SeDevice::load(&fx.dir).unwrap();
    assert!(me.agreement_discarded());
    assert!(vault_helper::device::se::agreement_public(me.key_tag()).is_err(), "no agreement key exists");
    let _ = fx.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    let la = std::sync::Arc::new(La { allow: true, calls: std::sync::atomic::AtomicUsize::new(0) });
    fx.set_presence(la.clone());
    let r = fx.op(json!({"op": "unlock"}));
    assert_eq!(err_code(&r), "DEVICE_NOT_AUTHORIZED", "{r}");
    assert_eq!(la.calls.load(std::sync::atomic::Ordering::SeqCst), 0, "no prompt before the password");
    assert_eq!(unlock(&fx, MP)["ok"], true);
    fx.remove_dir();
}

/// SEC-I6 / VER-I12: the revoker's cutoff lives only in the rotation's
/// staged DB — a revocation whose rotation fails (here just before the
/// commit marker) refuses nothing and leaves the phone's held puts.
#[test]
fn a_failed_revocation_rotation_refuses_nothing() {
    use vault_helper::storage::rotation::{rotate, MpWrap, RkWrap};
    use vault_helper::storage::rotation_journal::FailAt;
    let _g = serial();
    let w = world("fu-failrot");
    let c = ctx(&w, true, false, false);
    let (dir, mut ps, k) = phone_store(&w);
    let rid = ps.add_record(&k, 1, PT, META).unwrap();
    let row = row_of(&ps, &rid);
    drop(ps);
    let _ = std::fs::remove_dir_all(dir);
    assert_eq!(ask(&w, &c, PeerOp::RevsPut, put_body(&[row.clone()])).0, PeerStatus::Ok);
    let mut store = VaultStore::open(&w.fx.dir).unwrap();
    vault_helper::peer::inbox::stash(&store, &w.id, &put_body(&[row.clone()]), NOW).unwrap();
    let cutoff = set_aside::only_from(&store.conn, &w.id, &Default::default()).unwrap();
    assert_eq!(cutoff.len(), 1);
    let vk = vk(&w);
    set_aside::set_aside(&mut store, &vk).unwrap();
    let me = vault_helper::storage::revisions::uuid_string(&vault_helper::registry::device::DeviceIdentity::device_id(&c.me));
    let plan = vault_helper::device::rotate::EnvelopePlan { vault_id: c.vault_id, devices: vec![], fresh: vec![], commit_tag: None };
    let change = vault_helper::sync::change::RemoteChange {
        envelopes: Some(&plan),
        op: vault_helper::sync::pending::PendingOp::Revocation,
        security_driven: true,
        base: vault_helper::sync::pending::Base::of(&store.header),
        mp: None,
        rk: None,
        revoke: Some((w.id, me)),
        registry: None,
        cutoff,
    };
    assert!(rotate(store, &vk, MpWrap::Fresh(MP), RkWrap::Remove, Some(&change), Some(FailAt::AfterHeadStaged)).is_err());
    let store = VaultStore::open(&w.fx.dir).unwrap();
    assert!(!set_aside::refused(&store.conn, &row.revision_id).unwrap(), "nothing refused for good");
    let held: i64 = store.conn.query_row("SELECT count(*) FROM peer_inbox", [], |r| r.get(0)).unwrap();
    assert_eq!(held, 1, "the phone's held put is still there");
    w.fx.remove_dir();
}
