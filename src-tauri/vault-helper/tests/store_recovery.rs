//! Store-level robustness from the milestone-2 review: a resolution over
//! more heads than one revision may name (SEC-I3), the crash window
//! between the DB commit and the manifest flip (SEC-I7), and a forged
//! higher-generation copy of a known revision (SEC-B1). Real on-disk
//! vaults, synthetic data only.

use std::path::PathBuf;

use vault_helper::backup::object;
use vault_helper::crypto::secret::{random_secret, SecretBytes};
use vault_helper::errors::ErrorCode;
use vault_helper::registry::device::{SoftwareDevice, PLATFORM_MACOS};
use vault_helper::storage::merge::{apply_batch, NoCompare};
use vault_helper::storage::revision_rows::all_rows;
use vault_helper::storage::revisions::{heads, new_revision_id};
use vault_helper::VAULT_HEADER_NAME;
use vault_helper::storage::manifest::MANIFEST_NAME;
use vault_helper::storage::store_records::Resolution;
use vault_helper::storage::VaultStore;
use vault_helper::vault::create::create_vault;

fn vault(tag: &str) -> (PathBuf, VaultStore, SecretBytes<32>, String) {
    let d = std::env::temp_dir().join(format!("vhsr-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let dev = SoftwareDevice::generate("Synthetic Mac", PLATFORM_MACOS);
    let (_, vk) = create_vault(&d, b"synthetic-store-recovery-password-01", &random_secret(), &dev).unwrap();
    let mut s = VaultStore::open(&d).unwrap();
    let r = s.add_record(&vk, 1, br#"{"password":"synthetic-base"}"#, br#"{"title":"base","hosts":[]}"#).unwrap();
    (d, s, vk, r)
}

/// Admit rows the way sync does (objects → merge at the local generation).
fn admit(s: &mut VaultStore, rows: &[vault_helper::storage::revisions::RevisionRow]) {
    let rows: Vec<_> = rows.iter().map(|r| object::decode(&object::encode(r).unwrap()).unwrap()).collect();
    let gen = s.header.vk_generation;
    let tx = s.conn.transaction().unwrap();
    apply_batch(&tx, &rows, gen, &NoCompare).unwrap();
    tx.commit().unwrap();
    s.persist_head().unwrap();
}

/// SEC-I3: nine concurrent heads — one resolution covers eight, the
/// ninth stays in conflict, a second resolution finishes.
#[test]
fn nine_heads_resolve_in_two_steps() {
    let (d, mut s, vk, r) = vault("nine");
    let base = all_rows(&s.conn).unwrap().remove(0);
    // Nine sibling heads by nine other authors. Their ciphertexts are
    // copies and do not open; the `Edited` path never decrypts its base.
    let mut sibs = Vec::new();
    for i in 0..9u8 {
        let mut x = base.clone();
        x.revision_id = new_revision_id();
        x.parent_ids = vec![base.revision_id];
        x.author_device = format!("c{i}000000-0000-4000-8000-00000000000c");
        x.counter = 1;
        sibs.push(x);
    }
    admit(&mut s, &sibs);
    assert_eq!(heads(&s.conn, &r).unwrap().len(), 9);
    let pt = br#"{"password":"synthetic-merged"}"#;
    let meta = br#"{"title":"merged","hosts":[]}"#;
    let h0 = heads(&s.conn, &r).unwrap()[0];
    s.resolve(&vk, &r, Resolution::Edited { base: h0, kind_tag: 1, schema_version: 1, plaintext: pt, meta }, false).unwrap();
    assert_eq!(heads(&s.conn, &r).unwrap().len(), 2, "eight covered, one left in conflict");
    let h = heads(&s.conn, &r).unwrap();
    let mine = *h.iter().find(|x| !sibs.iter().any(|s| &s.revision_id == *x)).unwrap();
    s.resolve(&vk, &r, Resolution::Edited { base: mine, kind_tag: 1, schema_version: 1, plaintext: pt, meta }, false).unwrap();
    assert_eq!(heads(&s.conn, &r).unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}

/// SEC-I7: the DB committed, the manifest flip was lost → the one pending
/// step rolls forward; a manifest two steps behind still fails closed.
#[test]
fn crash_before_manifest_flip_rolls_forward() {
    let (d, mut s, vk, r) = vault("flip");
    let old = |n: &str| std::fs::read(d.join(n)).unwrap();
    let (m1, h1) = (old(MANIFEST_NAME), old(VAULT_HEADER_NAME));
    s.write_successor(&vk, &r, 1, 1, br#"{"password":"synthetic-2"}"#, br#"{"title":"2","hosts":[]}"#, 1).unwrap();
    drop(s);
    std::fs::write(d.join(MANIFEST_NAME), &m1).unwrap();
    std::fs::write(d.join(VAULT_HEADER_NAME), &h1).unwrap();
    let s = VaultStore::open(&d).expect("rolled forward");
    assert_eq!(all_rows(&s.conn).unwrap().len(), 2);
    let mut s = s;
    s.write_successor(&vk, &r, 1, 1, br#"{"password":"synthetic-3"}"#, br#"{"title":"3","hosts":[]}"#, 1).unwrap();
    s.write_successor(&vk, &r, 1, 1, br#"{"password":"synthetic-4"}"#, br#"{"title":"4","hosts":[]}"#, 1).unwrap();
    drop(s);
    std::fs::write(d.join(MANIFEST_NAME), &m1).unwrap();
    std::fs::write(d.join(VAULT_HEADER_NAME), &h1).unwrap();
    assert_eq!(VaultStore::open(&d).err(), Some(ErrorCode::ManifestMismatch), "not a single pending step");
    let _ = std::fs::remove_dir_all(&d);
}

/// SEC-B1: a same-graph copy at a far higher generation, with different
/// content, is never stored, and the vault still opens.
#[test]
fn higher_generation_forgery_is_inert() {
    let (d, mut s, vk, r) = vault("gen");
    let mut forged = all_rows(&s.conn).unwrap().remove(0);
    forged.vk_generation = u32::MAX;
    forged.ct = b"attacker-content".to_vec();
    admit(&mut s, &[forged]);
    drop(s);
    let s = VaultStore::open(&d).expect("still opens");
    assert!(s.read_tip(&vk, &r).is_ok());
    let _ = std::fs::remove_dir_all(&d);
}
