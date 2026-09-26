//! SY-07 (OV0OBJ02 parse rejections) and the §2.6 graph binding
//! (VER-I3/I4, SEC-I8): every graph field of a real sealed revision is
//! bound by its AAD, and `graph_digest` matches vectors computed
//! independently of this code. Synthetic data only.

use vault_helper::backup::object::{decode, encode, MAX_OBJECT};
use vault_helper::crypto::hex;
use vault_helper::crypto::secret::random_secret;
use vault_helper::errors::ErrorCode;
use vault_helper::registry::device::{SoftwareDevice, PLATFORM_MACOS};
use vault_helper::storage::revision_rows::all_rows;
use vault_helper::storage::revisions::{graph_digest, RevisionRow};
use vault_helper::storage::VaultStore;
use vault_helper::vault::create::create_vault;

/// Expected values computed with Python's hashlib over the §2.6 byte
/// layout, not with `graph_digest` itself.
#[test]
fn graph_digest_vectors() {
    let rid = hex::decode_array::<16>("5e7e0000000040008000000000000001").unwrap();
    let author = hex::decode_array::<16>("a000000000004000800000000000000a").unwrap();
    let d = graph_digest(&rid, &[0x11; 32], &author, 7, true, 2, &[[0x22; 32], [0x33; 32]]);
    assert_eq!(hex::encode(d), "9db91adc2484aa29a067860606c7ea28a03993969e884a2256f88587e3cff562");
    let d = graph_digest(&rid, &[0x11; 32], &author, 1, false, 1, &[]);
    assert_eq!(hex::encode(d), "b027d934a12c1e75554d1163f573c6468aa5a4d0b665a1c6a02fd346ff68ae88");
}

fn sample() -> RevisionRow {
    RevisionRow {
        revision_id: [0x44; 32],
        record_id: "5e7e0000-0000-4000-8000-000000000001".into(),
        parent_ids: vec![[0x10; 32], [0x20; 32]],
        author_device: "a0000000-0000-4000-8000-00000000000a".into(),
        counter: 3,
        deleted: false,
        kind_tag: 1,
        vk_generation: 1,
        schema_version: 1,
        nonce: [1; 24],
        ct: b"synthetic-ct".to_vec(),
        meta_nonce: [2; 24],
        meta_ct: b"synthetic-meta".to_vec(),
        created_at: 10,
        updated_at: 11,
    }
}

/// SY-07: byte-exact round trip; every malformed form is refused.
#[test]
fn sy07_object_parse_rejections() {
    let row = sample();
    let bytes = encode(&row).unwrap();
    assert_eq!(decode(&bytes).unwrap(), row);
    assert_eq!(encode(&decode(&bytes).unwrap()).unwrap(), bytes);
    let with = |f: &dyn Fn(&mut Vec<u8>)| {
        let mut b = bytes.clone();
        f(&mut b);
        decode(&b)
    };
    assert_eq!(with(&|b| b[..8].copy_from_slice(b"OV0OBJ01")), Err(ErrorCode::FormatInvalid), "retired magic");
    assert_eq!(with(&|b| b[..8].copy_from_slice(b"OV0OBJ03")), Err(ErrorCode::FormatTooNew), "unknown magic");
    assert_eq!(with(&|b| b[8] = 0), Err(ErrorCode::FormatInvalid), "kind 0");
    assert_eq!(with(&|b| b[8] = 0x7e), Err(ErrorCode::FormatTooNew), "unknown kind");
    assert_eq!(with(&|b| b[9] = 2), Err(ErrorCode::FormatInvalid), "reserved flag bit");
    assert_eq!(with(&|b| b[14] = 0x80), Err(ErrorCode::FormatInvalid), "counter > i64::MAX");
    assert_eq!(with(&|b| b[22..38].fill(0)), Err(ErrorCode::FormatInvalid), "zero author");
    assert_eq!(with(&|b| b[86] = 9), Err(ErrorCode::FormatInvalid), "parent_count 9");
    assert_eq!(with(&|b| { b[87..119].fill(0x20); }), Err(ErrorCode::FormatInvalid), "duplicate parents");
    assert_eq!(with(&|b| { b[87..119].fill(0x30); }), Err(ErrorCode::FormatInvalid), "unsorted parents");
    assert_eq!(with(&|b| b.push(0)), Err(ErrorCode::FormatInvalid), "trailing byte");
    assert_eq!(with(&|b| b.truncate(b.len() - 1)), Err(ErrorCode::FormatInvalid), "truncated");
    let mut big = sample();
    big.ct = vec![0; MAX_OBJECT];
    assert!(encode(&big).is_err(), "over 1 MiB");
    let mut oversized = bytes.clone();
    oversized.resize(MAX_OBJECT + 1, 0);
    assert_eq!(decode(&oversized), Err(ErrorCode::FormatInvalid));
}

/// VER-I4: changing any graph field of a real sealed revision makes both
/// its record and its metadata ciphertext fail to open.
#[test]
fn every_graph_field_is_bound() {
    let d = std::env::temp_dir().join(format!("vhbind-{}-{}", std::process::id(), random_secret().expose()[0]));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let dev = SoftwareDevice::generate("Synthetic Mac", PLATFORM_MACOS);
    let (_, vk) = create_vault(&d, b"synthetic-binding-master-password-01", &random_secret(), &dev).unwrap();
    let mut store = VaultStore::open(&d).unwrap();
    let r = store.add_record(&vk, 1, br#"{"password":"synthetic-pw"}"#, br#"{"title":"t","hosts":[]}"#).unwrap();
    store.write_successor(&vk, &r, 1, 1, br#"{"password":"synthetic-pw2"}"#, br#"{"title":"t2","hosts":[]}"#, 1).unwrap();
    let row = all_rows(&store.conn).unwrap().into_iter().find(|x| !x.parent_ids.is_empty()).unwrap();
    assert!(store.open_row(&vk, &row).is_ok() && store.open_row_meta(&vk, &row).is_ok());
    let tampers: Vec<(&str, Box<dyn Fn(&mut RevisionRow)>)> = vec![
        ("revision_id", Box::new(|x| x.revision_id[0] ^= 1)),
        ("record_id", Box::new(|x| x.record_id = "5e7e0000-0000-4000-8000-0000000000ff".into())),
        ("author", Box::new(|x| x.author_device = "b0000000-0000-4000-8000-00000000000b".into())),
        ("counter", Box::new(|x| x.counter += 1)),
        ("deleted", Box::new(|x| x.deleted = true)),
        ("kind", Box::new(|x| x.kind_tag = 2)),
        ("parents", Box::new(|x| x.parent_ids = vec![[0x77; 32]])),
        ("no parents", Box::new(|x| x.parent_ids.clear())),
    ];
    for (name, t) in tampers {
        let mut x = row.clone();
        t(&mut x);
        assert_eq!(store.open_row(&vk, &x).err(), Some(ErrorCode::RecordCorrupt), "{name}: record");
        assert!(store.open_row_meta(&vk, &x).is_err(), "{name}: meta");
    }
    drop(store);
    let _ = std::fs::remove_dir_all(&d);
}
