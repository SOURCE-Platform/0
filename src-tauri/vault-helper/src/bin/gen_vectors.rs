//! Deterministic vector generator (spec §16.8).
//!
//!   gen_vectors            write tests/vectors/*.json
//!   gen_vectors --check    fail (exit 1) if any vector file is stale
//!
//! Vectors are committed; CI runs `--check` so a crypto/wire change
//! without a regenerated vector set fails the gate.

use std::path::PathBuf;
use std::process::ExitCode;


fn vectors_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("vectors")
}

fn main() -> ExitCode {
    let check = std::env::args().nth(1).as_deref() == Some("--check");
    let dir = vectors_dir();
    let mut stale = Vec::new();
    for (stem, doc) in vault_helper::all_vectors() {
        let rendered = serde_json::to_string_pretty(&doc).unwrap() + "\n";
        let path = dir.join(format!("{stem}.json"));
        let existing = std::fs::read_to_string(&path).ok();
        if existing.as_deref() != Some(rendered.as_str()) {
            if check {
                stale.push(stem);
            } else {
                std::fs::create_dir_all(&dir).expect("create vectors dir");
                std::fs::write(&path, rendered).expect("write vector file");
                eprintln!("wrote {}", path.display());
            }
        }
    }
    if check && !stale.is_empty() {
        eprintln!(
            "stale vectors: {} — run `cargo run -p source-vault-helper --bin gen_vectors` (spec §16.8)",
            stale.join(", ")
        );
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
