//! VK rotation engine (spec §2.10, CR-08 at store level, BK-10 local
//! analog). Real on-disk vaults, production Argon2id tuple, synthetic
//! credentials only. No secret value is ever printed.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use vault_helper::crypto::kdf;
use vault_helper::crypto::record::{self, RecordCiphertext};
use vault_helper::crypto::secret::{random_secret, SecretBytes};
use vault_helper::crypto::wrap::{self, PasswordWrapFile, RecoveryWrapFile};
use vault_helper::registry::device::{SoftwareDevice, PLATFORM_MACOS};
use vault_helper::storage::import_log;
use vault_helper::storage::revision_rows::all_rows;
use vault_helper::storage::revisions::uuid_bytes;
use vault_helper::storage::rotation::{rotate, MpWrap, RkWrap};
use vault_helper::storage::rotation_journal::{FailAt, COMMIT_MARKER};
use vault_helper::storage::store::{PASSWORD_WRAP_NAME, RECOVERY_WRAP_NAME};
use vault_helper::storage::VaultStore;
use vault_helper::vault::create::create_vault;

const MP: &[u8] = b"synthetic-rotation-master-password-0001";
const MP_NEW: &[u8] = b"synthetic-rotation-master-password-0002";

fn dir() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = PathBuf::from(format!("/tmp/vhrot-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn pk_for(dir: &Path, mp: &[u8]) -> SecretBytes<32> {
    let file = mp_wrap(dir);
    let salt: [u8; 16] = vault_helper::crypto::hex::decode_array(&file.argon2id.salt).unwrap();
    let params = kdf::Argon2Params { m: file.argon2id.m, t: file.argon2id.t, p: file.argon2id.p };
    kdf::derive_pk(mp, &salt, params).unwrap()
}

fn mp_wrap(dir: &Path) -> PasswordWrapFile {
    serde_json::from_slice(&std::fs::read(dir.join(PASSWORD_WRAP_NAME)).unwrap()).unwrap()
}

fn rk_wrap(dir: &Path) -> Option<RecoveryWrapFile> {
    std::fs::read(dir.join(RECOVERY_WRAP_NAME)).ok().map(|b| serde_json::from_slice(&b).unwrap())
}

/// A vault with history: 3 logins, one updated twice, one deleted, plus
/// two import_log fingerprints.
fn seeded() -> (PathBuf, SecretBytes<32>, SecretBytes<32>, Vec<String>) {
    let d = dir();
    let rk = random_secret();
    let dev = SoftwareDevice::generate("Synthetic Mac", PLATFORM_MACOS);
    let (_, vk) = create_vault(&d, MP, &rk, &dev).unwrap();
    let mut store = VaultStore::open(&d).unwrap();
    let mut refs = Vec::new();
    for i in 0..3 {
        let meta = format!(r#"{{"title":"Synthetic {i}","username":"u{i}@example.test","hosts":["h{i}.example.test"]}}"#);
        let pt = format!(r#"{{"password":"synthetic-pw-{i}"}}"#);
        refs.push(store.add_record(&vk, 1, pt.as_bytes(), meta.as_bytes()).unwrap());
    }
    for v in 0..2 {
        let pt = format!(r#"{{"password":"synthetic-pw-0-v{v}"}}"#);
        store.write_successor(&vk, &refs[0], 1, 1, pt.as_bytes(), br#"{"title":"Synthetic 0"}"#, 0).unwrap();
    }
    store.tombstone(&vk, &refs[2]).unwrap();
    for identity in [&b"\x01example.test\x00u0"[..], &b"\x01example.test\x00u1"[..]] {
        let (fp, ct) = import_log::seal_identity(&vk, &store.header, identity).unwrap();
        store
            .conn
            .execute("INSERT INTO import_log (fingerprint, identity_ct) VALUES (?1, ?2)", rusqlite::params![fp.as_slice(), ct])
            .unwrap();
    }
    (d, vk, rk, refs)
}

fn rows_open_under(store: &VaultStore, vk: &SecretBytes<32>) -> (usize, usize) {
    let (mut ok, mut fail) = (0, 0);
    for row in all_rows(&store.conn).unwrap() {
        let rid = uuid_bytes(&row.record_id).unwrap();
        let sealed = RecordCiphertext { nonce: row.nonce, ct: row.ct.clone() };
        match record::open_record(vk, &store.header.vault_id.0, &rid, &row.bind().unwrap(), row.schema_version, row.vk_generation, &sealed) {
            Ok(_) => ok += 1,
            Err(_) => fail += 1,
        }
    }
    (ok, fail)
}

#[test]
fn rotation_reseals_everything_and_old_vk_fails_everywhere() {
    let (d, old_vk, rk, refs) = seeded();
    let before = VaultStore::open(&d).unwrap();
    let graph_before: Vec<_> = all_rows(&before.conn).unwrap().iter().map(|r| (r.revision_id, r.parent_ids.clone(), r.counter)).collect();
    let (rev_count, tips, gen_before) = (graph_before.len(), before.manifest.item_count, before.header.manifest_generation);
    let pk = pk_for(&d, MP);
    let out = rotate(before, &old_vk, MpWrap::Reseal(&pk), RkWrap::Seal(&rk), None, None).unwrap();
    assert_ne!(out.new_vk.expose(), old_vk.expose());
    assert_eq!(out.vk_generation, 2);

    let after = VaultStore::open(&d).unwrap(); // §3.5 manifest == DB still holds
    assert_eq!(after.header.vk_generation, 2);
    assert_eq!(after.header.manifest_generation, gen_before + 1);
    let rows = all_rows(&after.conn).unwrap();
    assert_eq!(rows.len(), rev_count, "history shape preserved");
    assert!(rows.iter().all(|r| r.vk_generation == 2));
    // SY-10 (v0.4): ids, parents and counters are unchanged — only the
    // ciphertexts and vk_generation moved.
    let graph_after: Vec<_> = rows.iter().map(|r| (r.revision_id, r.parent_ids.clone(), r.counter)).collect();
    assert_eq!(graph_after, graph_before, "rotation never renames the revision graph");
    assert_eq!(after.manifest.item_count, tips);
    assert_eq!(rows_open_under(&after, &out.new_vk), (rev_count, 0), "every revision opens under the new VK");
    assert_eq!(rows_open_under(&after, &old_vk), (0, rev_count), "old VK fails on every revision");
    let tip = after.read_tip(&out.new_vk, &refs[0]).unwrap();
    assert!(std::str::from_utf8(&tip.plaintext).unwrap().contains("v1"), "latest revision content kept");
    assert!(after.read_tip(&out.new_vk, &refs[2]).is_err(), "tombstone kept");
    assert_eq!(after.list_records(&out.new_vk).unwrap().len(), 2);

    // Wraps rebuilt: MP (same PK) and RK both yield exactly the new VK.
    let mp_payload = wrap::open_wrap_mp(&mp_wrap(&d), &pk, &after.header.vault_id.0).unwrap();
    assert_eq!(mp_payload.vk.expose(), out.new_vk.expose());
    assert_eq!(mp_payload.vk_generation, 2);
    let rk_payload = wrap::open_wrap_rk(&rk_wrap(&d).unwrap(), &rk, &after.header.vault_id.0).unwrap();
    assert_eq!(rk_payload.vk.expose(), out.new_vk.expose());

    // Import fingerprints recomputed under the new key; identities intact.
    let mut stmt = after.conn.prepare("SELECT fingerprint, identity_ct FROM import_log").unwrap();
    let rows: Vec<(Vec<u8>, Vec<u8>)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(rows.len(), 2);
    for (fp, ct) in rows {
        let identity = import_log::open_identity(&out.new_vk, &after.header, &ct).unwrap();
        let (expected, _) = import_log::seal_identity(&out.new_vk, &after.header, &identity).unwrap();
        assert_eq!(fp, expected.to_vec(), "fingerprint recomputed under the new generation key");
        let (old_fp, _) = import_log::seal_identity(&old_vk, &after.header, &identity).unwrap();
        assert_ne!(fp, old_fp.to_vec());
        assert!(import_log::open_identity(&old_vk, &after.header, &ct).is_err());
    }
    std::fs::remove_dir_all(&d).ok();
}

/// Crash after each staging step: the vault must open entirely old (no
/// marker yet) or entirely new (marker written), never half-rotated, with
/// no staged file left behind.
#[test]
fn crash_at_every_step_leaves_old_or_new_never_half() {
    for (fail, expect_new) in [
        (FailAt::AfterDbStaged, false),
        (FailAt::AfterWrapsStaged, false),
        (FailAt::AfterHeadStaged, false),
        (FailAt::AfterMarker, true),
        (FailAt::AfterFirstRename, true),
    ] {
        let (d, old_vk, rk, _) = seeded();
        let store = VaultStore::open(&d).unwrap();
        let revs = all_rows(&store.conn).unwrap().len();
        let pk = pk_for(&d, MP);
        assert!(rotate(store, &old_vk, MpWrap::Reseal(&pk), RkWrap::Seal(&rk), None, Some(fail)).is_err());

        let reopened = VaultStore::open(&d).unwrap_or_else(|e| panic!("{fail:?}: open failed {e:?}"));
        assert!(!d.join(COMMIT_MARKER).exists(), "{fail:?}: marker left behind");
        let leftovers: Vec<_> = walk(&d).into_iter().filter(|p| p.to_string_lossy().ends_with(".next")).collect();
        assert!(leftovers.is_empty(), "{fail:?}: staged files left: {leftovers:?}");
        let vk = wrap::open_wrap_mp(&mp_wrap(&d), &pk, &reopened.header.vault_id.0).unwrap().vk;
        let gen = reopened.header.vk_generation;
        assert_eq!(gen, if expect_new { 2 } else { 1 }, "{fail:?}");
        assert_eq!(expect_new, vk.expose() != old_vk.expose(), "{fail:?}: wrap/VK disagree");
        assert_eq!(rows_open_under(&reopened, &vk), (revs, 0), "{fail:?}: records vs accepted VK");
        let rk_vk = wrap::open_wrap_rk(&rk_wrap(&d).unwrap(), &rk, &reopened.header.vault_id.0).unwrap().vk;
        assert_eq!(rk_vk.expose(), vk.expose(), "{fail:?}: RK wrap matches accepted state");
        std::fs::remove_dir_all(&d).ok();
    }
}

fn walk(d: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() { out.extend(walk(&p)); } else { out.push(p); }
    }
    out
}

/// New MP (RK recovery / MP set) + new RK (RK rotation): old MP and old
/// RK fail on the new state; the pre-rotation copy still opens with the
/// old RK (BK-10 at storage level: historical snapshots are not erased).
#[test]
fn fresh_mp_new_rk_and_historical_snapshot() {
    let (d, old_vk, old_rk, refs) = seeded();
    let snapshot = dir();
    for name in [PASSWORD_WRAP_NAME, RECOVERY_WRAP_NAME, "header.json", "manifest.json", "vault.db"] {
        let _ = std::fs::create_dir_all(snapshot.join("wraps"));
        std::fs::copy(d.join(name), snapshot.join(name)).unwrap();
    }
    let new_rk = random_secret();
    let store = VaultStore::open(&d).unwrap();
    let out = rotate(store, &old_vk, MpWrap::Fresh(MP_NEW), RkWrap::Seal(&new_rk), None, None).unwrap();
    let after = VaultStore::open(&d).unwrap();
    let id = after.header.vault_id.0;
    assert!(wrap::open_wrap_mp(&mp_wrap(&d), &pk_for(&d, MP), &id).is_err(), "old MP dead");
    assert_eq!(wrap::open_wrap_mp(&mp_wrap(&d), &pk_for(&d, MP_NEW), &id).unwrap().vk.expose(), out.new_vk.expose());
    assert!(wrap::open_wrap_rk(&rk_wrap(&d).unwrap(), &old_rk, &id).is_err(), "new state refuses old RK");
    assert_eq!(wrap::open_wrap_rk(&rk_wrap(&d).unwrap(), &new_rk, &id).unwrap().vk.expose(), out.new_vk.expose());
    // Historical copy: old RK + old wrap + old bytes still decrypt.
    let hist_vk = wrap::open_wrap_rk(&rk_wrap(&snapshot).unwrap(), &old_rk, &id).unwrap().vk;
    let hist = VaultStore::open(&snapshot).unwrap();
    assert!(hist.read_tip(&hist_vk, &refs[1]).is_ok(), "historical snapshot decrypts (documented limitation)");
    std::fs::remove_dir_all(&d).ok();
    std::fs::remove_dir_all(&snapshot).ok();
}

#[test]
fn rk_remove_plan_deletes_recovery_wrap() {
    let (d, old_vk, _, _) = seeded();
    let pk = pk_for(&d, MP);
    let store = VaultStore::open(&d).unwrap();
    rotate(store, &old_vk, MpWrap::Reseal(&pk), RkWrap::Remove, None, None).unwrap();
    assert!(rk_wrap(&d).is_none(), "no wrap of the old VK survives");
    VaultStore::open(&d).unwrap();
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn reseal_with_wrong_pk_refuses_before_staging_anything() {
    let (d, old_vk, rk, _) = seeded();
    let store = VaultStore::open(&d).unwrap();
    let wrong = random_secret();
    assert!(rotate(store, &old_vk, MpWrap::Reseal(&wrong), RkWrap::Seal(&rk), None, None).is_err());
    let reopened = VaultStore::open(&d).unwrap();
    assert_eq!(reopened.header.vk_generation, 1);
    std::fs::remove_dir_all(&d).ok();
}
