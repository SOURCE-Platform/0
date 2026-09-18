//! §3.2 merge-rule integration tests (moved out of `storage/revisions.rs`
//! under the repo's 350-line file cap; they exercise the public API only).

use rusqlite::{params, Connection};
use vault_helper::storage::{db, revisions::*};

fn setup(tag: &str) -> (std::path::PathBuf, Connection) {
    // unique per test invocation (tests run in parallel): pid + nanos
    let dir = std::env::temp_dir().join(format!(
        "vh-rev-{tag}-{}-{}",
        std::process::id(),
        now_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let conn = db::open_db(&dir.join(db::DB_NAME), true).unwrap();
    (dir, conn)
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn rev(record_id: &str, parents: &[[u8; 32]], counter: u64, deleted: bool, tag: u8) -> RevisionRow {
    let rid = uuid_bytes(record_id).unwrap();
    let dev = uuid_bytes(LOCAL_DEVICE_ID).unwrap();
    let ct = format!("ct-{counter}-{deleted}-{tag}").into_bytes();
    let meta_ct = b"meta".to_vec();
    let hash = rev_hash(&rid, parents, &dev, counter, deleted, &ct, &meta_ct);
    RevisionRow {
        rev_hash: hash,
        record_id: record_id.to_string(),
        parent_revs: parents.to_vec(),
        author_device: LOCAL_DEVICE_ID.to_string(),
        counter,
        deleted,
        kind_tag: 1,
        vk_generation: 1,
        schema_version: 1,
        nonce: [0u8; 24],
        ct,
        meta_nonce: [0u8; 24],
        meta_ct,
        created_at: 1,
        updated_at: 1,
    }
}

#[test]
fn rev_hash_is_content_committed() {
    let rid = [1u8; 16];
    let dev = [0u8; 16];
    let a = rev_hash(&rid, &[], &dev, 1, false, b"x", b"y");
    assert_eq!(a, rev_hash(&rid, &[], &dev, 1, false, b"x", b"y"));
    assert_ne!(a, rev_hash(&rid, &[], &dev, 2, false, b"x", b"y"));
    assert_ne!(a, rev_hash(&rid, &[], &dev, 1, true, b"x", b"y"));
    assert_ne!(a, rev_hash(&rid, &[], &dev, 1, false, b"z", b"y"));
    assert_ne!(a, rev_hash(&[2u8; 16], &[], &dev, 1, false, b"x", b"y"));
}

#[test]
fn fast_forward_chain_and_duplicate() {
    let (dir, conn) = setup("ff");
    let rid = new_record_id();
    let r1 = rev(&rid, &[], 1, false, 0);
    assert_eq!(apply_revision(&conn, &r1).unwrap(), MergeOutcome::FastForward);
    assert_eq!(current_tip(&conn, &rid).unwrap(), Some(r1.rev_hash));
    let r2 = rev(&rid, &[r1.rev_hash], 2, false, 1);
    assert_eq!(apply_revision(&conn, &r2).unwrap(), MergeOutcome::FastForward);
    assert_eq!(current_tip(&conn, &rid).unwrap(), Some(r2.rev_hash));
    assert_eq!(apply_revision(&conn, &r2).unwrap(), MergeOutcome::AlreadyKnown);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn concurrent_edits_conflict_and_null_the_tip() {
    let (dir, conn) = setup("cc");
    let rid = new_record_id();
    let r1 = rev(&rid, &[], 1, false, 0);
    apply_revision(&conn, &r1).unwrap();
    // A "second device" branched at the same parent.
    let r2a = rev(&rid, &[r1.rev_hash], 2, false, 1);
    let mut r2b = rev(&rid, &[r1.rev_hash], 2, false, 2);
    r2b.author_device = "11111111-1111-4111-8111-111111111111".to_string();
    r2b.rev_hash = rev_hash(
        &uuid_bytes(&rid).unwrap(),
        &[r1.rev_hash],
        &uuid_bytes(&r2b.author_device).unwrap(),
        2,
        false,
        &r2b.ct,
        &r2b.meta_ct,
    );
    assert_eq!(apply_revision(&conn, &r2a).unwrap(), MergeOutcome::FastForward);
    assert_eq!(apply_revision(&conn, &r2b).unwrap(), MergeOutcome::Conflict);
    assert_eq!(current_tip(&conn, &rid).unwrap(), None);
    let n: i64 = conn
        .query_row(
            "SELECT count(*) FROM record_conflicts WHERE record_id=?1",
            params![rid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 2, "both branches held for user resolution");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn equivocation_conflicts_with_same_counter() {
    let (dir, conn) = setup("eq");
    let rid = new_record_id();
    let r1 = rev(&rid, &[], 1, false, 0);
    apply_revision(&conn, &r1).unwrap();
    let forged = rev(&rid, &[], 1, false, 9); // same counter, new content
    assert_ne!(forged.rev_hash, r1.rev_hash);
    assert_eq!(apply_revision(&conn, &forged).unwrap(), MergeOutcome::Conflict);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn edit_after_tombstone_never_resurrects() {
    let (dir, conn) = setup("ts");
    let rid = new_record_id();
    let r1 = rev(&rid, &[], 1, false, 0);
    let r2 = rev(&rid, &[r1.rev_hash], 2, true, 1); // tombstone, fast-forward
    apply_revision(&conn, &r1).unwrap();
    assert_eq!(apply_revision(&conn, &r2).unwrap(), MergeOutcome::FastForward);
    let r3 = rev(&rid, &[r2.rev_hash], 3, false, 2); // edit claiming ancestry past tombstone
    assert_eq!(apply_revision(&conn, &r3).unwrap(), MergeOutcome::Conflict);
    std::fs::remove_dir_all(dir).ok();
}