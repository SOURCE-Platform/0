//! Phase I (spec §10): `import_dashlane` over the synthetic fixtures that
//! `scripts/gen-dashlane-fixtures.py` writes. Synthetic data only — no
//! real export is ever read by a test.

mod vault_fx;

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use serde_json::{json, Value};
use vault_fx::*;

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dashlane").join(name).to_string_lossy().into_owned()
}

fn import(fx: &Fx, path: &str) -> Value {
    let r = fx.op(json!({"op": "import_dashlane", "path": path}));
    assert_eq!(r["ok"], true, "{r}");
    let report = r["report"].clone();
    // §10.5: counts, categories, row numbers — never a value.
    let text = report.to_string();
    for leak in ["fixture-", "example.test", "Site ", "Fixture", "4111", "Testville"] {
        assert!(!text.contains(leak), "the report shows {leak:?}: {text}");
    }
    report
}

fn counts(r: &Value) -> (u64, u64, u64) {
    (r["imported"].as_u64().unwrap(), r["duplicates"].as_u64().unwrap(), r["skipped_malformed"].as_u64().unwrap())
}

fn kinds(fx: &Fx) -> Vec<String> {
    let items = fx.op(json!({"op": "list_items"}))["items"].as_array().unwrap().clone();
    let mut k: Vec<String> = items.iter().map(|i| i["kind"].as_str().unwrap().to_string()).collect();
    k.sort();
    k
}

#[test]
fn the_mixed_zip_imports_every_type_once() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let calls = fx.la.calls.load(Ordering::SeqCst);
    let r = import(&fx, &fixture("mixed.zip"));
    assert_eq!(fx.la.calls.load(Ordering::SeqCst), calls + 1, "one presence check for the whole import");
    assert_eq!(counts(&r), (7, 0, 0), "{r}");
    assert_eq!(r["cvv_discarded"], 3);
    assert_eq!(r["skipped_unsupported"]["member ids.csv"], 1, "{r}");
    assert_eq!(kinds(&fx), ["card", "card", "card", "login", "login", "secure_note", "secure_note"]);
    // A note's body stays in the ciphertext; reveal opens it.
    let items = fx.op(json!({"op": "list_items"}))["items"].as_array().unwrap().clone();
    let note = items.iter().find(|i| i["title"] == "Fixture note").unwrap();
    assert!(note.get("body").is_none() && note["hosts"] == json!([]), "metadata is the title only");
    let shown = fx.op(json!({"op": "reveal", "ref": note["ref"]}));
    assert_eq!(shown["secret"]["body"], "fixture body, with a comma\nand a second line", "{shown}");
    // The same file again: all duplicates, nothing new (§10.3).
    let again = import(&fx, &fixture("mixed.zip"));
    assert_eq!(counts(&again), (0, 7, 0), "{again}");
    assert_eq!(kinds(&fx).len(), 7);
    fx.remove_dir();
}

#[test]
fn row_rules_each_fixture() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let r = import(&fx, &fixture("embedded.csv"));
    assert_eq!(counts(&r), (2, 0, 0), "{r}");
    let r = import(&fx, &fixture("invalid-utf8.csv"));
    assert_eq!(counts(&r), (2, 0, 1), "{r}");
    assert_eq!(r["skipped_rows"][0], json!({"file": "csv", "row": 3, "why": "malformed"}));
    let r = import(&fx, &fixture("oversized.csv"));
    assert_eq!(counts(&r), (1, 0, 1), "{r}");
    assert_eq!(r["skipped_rows"][0]["row"], 2);
    let r = import(&fx, &fixture("duplicate-storm.csv"));
    assert_eq!(counts(&r), (1, 49, 0), "{r}");
    let r = import(&fx, &fixture("otp-and-short.csv"));
    assert_eq!(counts(&r), (1, 0, 1), "{r}");
    assert_eq!(r["skipped_unsupported"]["otpSecret (re-enter manually)"], 1);
    assert_eq!(r["skipped_rows"][0], json!({"file": "csv", "row": 3, "why": "column count mismatch"}));
    for name in ["empty.csv", "unknown-columns.csv"] {
        let r = import(&fx, &fixture(name));
        assert_eq!(counts(&r), (0, 0, 0), "{name}: {r}");
        assert_eq!(r["skipped_unsupported"]["unrecognized columns in csv"], 1, "{name}: {r}");
    }
    assert_eq!(counts(&import(&fx, &fixture("header-only.csv"))), (0, 0, 0));
    // The embedded fixture's quoting survived into the record.
    let items = fx.op(json!({"op": "list_items"}))["items"].as_array().unwrap().clone();
    let comma = items.iter().find(|i| i["title"] == "Comma, Inc.").unwrap();
    assert_eq!(fx.op(json!({"op": "reveal", "ref": comma["ref"]}))["secret"]["password"], "fixture-\"q\",1");
    fx.remove_dir();
}

#[test]
fn an_item_already_in_the_vault_is_a_duplicate() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let r = fx.op(json!({"op": "add_item", "kind": "login", "title": "Mine", "username": "user1@example.test", "password": "synthetic-mine", "hosts": ["site1.example.test"]}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(counts(&import(&fx, &fixture("valid-minimal.csv"))), (1, 1, 0));
    let mine = fx.op(json!({"op": "reveal", "ref": r["ref"]}));
    assert_eq!(mine["secret"]["password"], "synthetic-mine", "the vault's item is kept");
    fx.remove_dir();
}

#[test]
fn bad_paths_are_refused_before_any_prompt() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let link = fx.dir.with_extension("link.csv");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(fixture("valid-minimal.csv"), &link).unwrap();
    let inside = fx.dir.join("export.csv");
    std::fs::copy(fixture("valid-minimal.csv"), &inside).unwrap();
    let calls = fx.la.calls.load(Ordering::SeqCst);
    for path in [
        "tests/fixtures/dashlane/valid-minimal.csv".to_string(),
        link.to_string_lossy().into_owned(),
        inside.to_string_lossy().into_owned(),
        fixture(""),
        fixture("missing.csv"),
    ] {
        let r = fx.op(json!({"op": "import_dashlane", "path": path}));
        assert_eq!(r["error"], "INVALID_INPUT", "{path}: {r}");
    }
    assert_eq!(fx.la.calls.load(Ordering::SeqCst), calls, "no presence prompt for a refused path");
    let _ = std::fs::remove_file(&link);
    fx.remove_dir();
}

#[test]
fn presence_refused_imports_nothing_and_locked_refuses() {
    let _g = serial();
    let mut fx = fx();
    setup_and_unlock(&fx);
    fx.set_presence(std::sync::Arc::new(La { allow: false, calls: Default::default() }));
    let r = fx.op(json!({"op": "import_dashlane", "path": fixture("mixed.zip")}));
    assert_eq!(r["error"], "PRESENCE_DENIED", "{r}");
    assert!(kinds(&fx).is_empty());
    fx.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    assert_eq!(fx.op(json!({"op": "import_dashlane", "path": fixture("mixed.zip")}))["error"], "BAD_STATE");
    fx.remove_dir();
}

/// §10.7: the 10^5-row stress file, written here (not committed). The
/// 60 s bound is for optimized code (16.5 s on 2026-10-17; unoptimized
/// it takes ~95 s), so this runs on demand:
/// `CARGO_TARGET_DIR=<scratch> CARGO_PROFILE_DEV_OPT_LEVEL=3 cargo test -p
/// source-vault-helper --test dashlane_import -- --ignored`
#[test]
#[ignore = "stress: run optimized, see the comment"]
fn a_hundred_thousand_rows_stream_in() {
    use std::io::Write;
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let path = fx.dir.with_extension("stress.csv");
    let mut f = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
    writeln!(f, "username,title,password,url").unwrap();
    let rows: u32 = std::env::var("OV0_STRESS_ROWS").ok().and_then(|v| v.parse().ok()).unwrap_or(100_000);
    for i in 0..rows {
        writeln!(f, "u{i}@example.test,S{i},fixture-{i:08x},https://s{}.example.test", i % 977).unwrap();
    }
    drop(f);
    let started = std::time::Instant::now();
    let r = import(&fx, &path.to_string_lossy());
    let took = started.elapsed();
    eprintln!("10^5 rows: {took:?}");
    assert_eq!(counts(&r), (rows as u64, 0, 0));
    assert!(took.as_secs() < 60, "{took:?}");
    let _ = std::fs::remove_file(&path);
    fx.remove_dir();
}

/// §10.8 Choice A: a note added by hand — title-only metadata, body on
/// reveal, oversize refused, never a website candidate.
#[test]
fn a_secure_note_added_by_hand() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let r = fx.op(json!({"op": "add_item", "kind": "secure_note", "title": "Synthetic note", "body": "line 1\nline 2"}));
    assert_eq!(r["ok"], true, "{r}");
    let items = fx.op(json!({"op": "list_items"}))["items"].clone();
    assert_eq!(items[0]["kind"], "secure_note");
    assert!(!items.to_string().contains("line 1"), "the body is not metadata");
    assert_eq!(fx.op(json!({"op": "reveal", "ref": r["ref"]}))["secret"]["body"], "line 1\nline 2");
    let big = "x".repeat(64 * 1024 + 1);
    assert_eq!(fx.op(json!({"op": "add_item", "kind": "secure_note", "title": "Big", "body": big}))["error"], "INVALID_INPUT");
    assert_eq!(fx.op(json!({"op": "add_item", "kind": "secure_note", "title": " ", "body": "x"}))["error"], "INVALID_INPUT");
    let c = fx.op(json!({"op": "browser_candidates", "origin": "https://example.test", "tab_url": "https://example.test/"}));
    assert_eq!(c["accounts"], json!([]), "{c}");
    fx.remove_dir();
}
