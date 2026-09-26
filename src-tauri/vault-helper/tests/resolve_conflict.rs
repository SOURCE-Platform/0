//! §3.2 resolution at the store level (SY-08 local half): two devices edit
//! one record concurrently, sync through `OV0OBJ02` objects, the record
//! lists as conflicted with per-head versions, edits are refused, and
//! `resolve` collapses the heads; an author fork freezes and `resolve`
//! clears it. Real on-disk vaults, synthetic data only.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use vault_helper::backup::object;
use vault_helper::crypto::secret::{random_secret, SecretBytes};
use vault_helper::errors::ErrorCode;
use vault_helper::registry::device::{SoftwareDevice, PLATFORM_MACOS};
use vault_helper::storage::merge::{apply_batch, NoCompare};
use vault_helper::storage::rev_state::is_frozen;
use vault_helper::storage::revision_rows::all_rows;
use vault_helper::storage::revisions::heads;
use vault_helper::storage::store_records::Resolution;
use vault_helper::storage::VaultStore;
use vault_helper::vault::create::create_vault;

const MP: &[u8] = b"synthetic-resolve-master-password-0001";
const B_DEV: [u8; 16] = [0xb0; 16];

fn dir() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = PathBuf::from(format!("/tmp/vhres-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn copy_vault(from: &Path) -> PathBuf {
    let to = dir();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_file() {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
    to
}

fn edit(store: &mut VaultStore, vk: &SecretBytes<32>, r: &str, title: &str) -> Result<(), ErrorCode> {
    let meta = format!(r#"{{"title":"{title}","username":"u@example.test","hosts":[]}}"#);
    let pt = format!(r#"{{"password":"synthetic-{title}"}}"#);
    store.write_successor(vk, r, 1, 1, pt.as_bytes(), meta.as_bytes(), 1)
}

/// Pull every revision of `from` into `into` the way sync does: through
/// serialized objects and the §3.2 merge.
fn sync(into: &mut VaultStore, from: &VaultStore) {
    let rows: Vec<_> = all_rows(&from.conn)
        .unwrap()
        .iter()
        .map(|r| object::decode(&object::encode(r).unwrap()).unwrap())
        .collect();
    let tx = into.conn.transaction().unwrap();
    let gen = into.header.vk_generation;
    apply_batch(&tx, &rows, gen, &NoCompare).unwrap();
    tx.commit().unwrap();
    into.persist_head().unwrap();
}

/// A vault with one record on device A, plus a copy of it for a second
/// device (`b_author` picks whether the copy authors as B or as A).
fn two_devices(b_author: Option<[u8; 16]>) -> (VaultStore, VaultStore, SecretBytes<32>, String, [PathBuf; 2]) {
    let d = dir();
    let dev = SoftwareDevice::generate("Synthetic Mac", PLATFORM_MACOS);
    let (_, vk) = create_vault(&d, MP, &random_secret(), &dev).unwrap();
    let mut a = VaultStore::open(&d).unwrap();
    let r = a.add_record(&vk, 1, br#"{"password":"synthetic-base"}"#, br#"{"title":"base","hosts":[]}"#).unwrap();
    drop(a);
    let d2 = copy_vault(&d);
    let b = VaultStore::open(&d2).unwrap();
    if let Some(id) = b_author {
        b.set_author_device(&id).unwrap();
    }
    (VaultStore::open(&d).unwrap(), b, vk, r, [d, d2])
}

fn title_of(store: &VaultStore, vk: &SecretBytes<32>, r: &str) -> String {
    let items = store.list_records(vk).unwrap();
    let item = items.iter().find(|i| i["ref"] == r).unwrap();
    item["title"].as_str().unwrap_or("").to_string()
}

#[test]
fn concurrent_edits_conflict_then_resolve_to_chosen_head() {
    let (mut a, mut b, vk, r, dirs) = two_devices(Some(B_DEV));
    edit(&mut a, &vk, &r, "from-a").unwrap();
    edit(&mut b, &vk, &r, "from-b").unwrap();
    sync(&mut a, &b);

    let hs = heads(&a.conn, &r).unwrap();
    assert_eq!(hs.len(), 2);
    let items = a.list_records(&vk).unwrap();
    let item = items.iter().find(|i| i["ref"] == r.as_str()).unwrap();
    assert_eq!(item["conflicted"], true);
    assert!(item.get("tamper").is_none(), "two authors is a conflict, not tamper");
    let versions = item["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
    let b_rev = versions.iter().find(|v| v["title"] == "from-b").unwrap()["rev"].as_str().unwrap().to_string();
    // Edits are refused while conflicted (fail closed, no silent pick).
    assert_eq!(edit(&mut a, &vk, &r, "blind"), Err(ErrorCode::ConflictPending));
    assert!(a.read_tip(&vk, &r).is_err());

    let chosen = vault_helper::crypto::hex::decode_array::<32>(&b_rev).unwrap();
    a.resolve(&vk, &r, Resolution::Chosen(chosen), false).unwrap();
    assert_eq!(heads(&a.conn, &r).unwrap().len(), 1);
    assert_eq!(title_of(&a, &vk, &r), "from-b");
    assert_eq!(a.read_tip(&vk, &r).unwrap().plaintext.as_slice(), br#"{"password":"synthetic-from-b"}"#);

    // The resolution syncs back and B converges to the same single head.
    sync(&mut b, &a);
    assert_eq!(heads(&b.conn, &r).unwrap(), heads(&a.conn, &r).unwrap());
    for d in dirs {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn resolve_with_edits_and_refusals() {
    let (mut a, mut b, vk, r, dirs) = two_devices(Some(B_DEV));
    edit(&mut a, &vk, &r, "from-a").unwrap();
    edit(&mut b, &vk, &r, "from-b").unwrap();
    sync(&mut a, &b);
    let hs = heads(&a.conn, &r).unwrap();
    // A non-head revision id is refused.
    assert_eq!(a.resolve(&vk, &r, Resolution::Chosen([0x55; 32]), false), Err(ErrorCode::InvalidInput));
    let pt = br#"{"password":"synthetic-merged"}"#;
    let meta = br#"{"title":"merged","hosts":[]}"#;
    let old = *all_rows(&a.conn).unwrap().iter().find(|x| x.parent_ids.is_empty()).map(|x| &x.revision_id).unwrap();
    let edited = |base| Resolution::Edited { base, kind_tag: 1, schema_version: 1, plaintext: pt, meta };
    assert_eq!(a.resolve(&vk, &r, edited(old), false), Err(ErrorCode::InvalidInput), "SEC-I6: base must be a head");
    a.resolve(&vk, &r, edited(hs[0]), false).unwrap();
    let now = heads(&a.conn, &r).unwrap();
    assert_eq!(now.len(), 1);
    assert!(!hs.contains(&now[0]));
    assert_eq!(title_of(&a, &vk, &r), "merged");
    // Nothing left to resolve on a single-head, unfrozen record.
    assert_eq!(a.resolve(&vk, &r, Resolution::Chosen(now[0]), false), Err(ErrorCode::BadState));
    for d in dirs {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn equivocation_freezes_until_acknowledged() {
    // The copy keeps A's author id: two A revisions with the same counter
    // that are not each other's ancestors — equivocation (§3.2 table).
    let (mut a, mut a2, vk, r, dirs) = two_devices(None);
    edit(&mut a, &vk, &r, "fork-1").unwrap();
    edit(&mut a2, &vk, &r, "fork-2").unwrap();
    sync(&mut a, &a2);
    assert!(is_frozen(&a.conn, &r).unwrap());
    let items = a.list_records(&vk).unwrap();
    let item = items.iter().find(|i| i["ref"] == r.as_str()).unwrap();
    assert_eq!(item["tamper"], true);
    assert_eq!(edit(&mut a, &vk, &r, "blind"), Err(ErrorCode::ConflictPending));

    let hs = heads(&a.conn, &r).unwrap();
    assert_eq!(a.resolve(&vk, &r, Resolution::Chosen(hs[0]), false), Err(ErrorCode::ConflictPending), "VER-I5: ack required");
    assert!(is_frozen(&a.conn, &r).unwrap());
    a.resolve(&vk, &r, Resolution::Chosen(hs[0]), true).unwrap();
    assert!(!is_frozen(&a.conn, &r).unwrap());
    assert_eq!(heads(&a.conn, &r).unwrap().len(), 1);
    edit(&mut a, &vk, &r, "after").unwrap();
    assert_eq!(title_of(&a, &vk, &r), "after");
    for d in dirs {
        let _ = std::fs::remove_dir_all(d);
    }
}
