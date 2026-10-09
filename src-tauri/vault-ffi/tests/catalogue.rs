//! FFI-01 (spec v0.5 §22.2): the C ABI is exactly the catalogue — in the
//! sources and in the built iOS library — the engine imports exactly the
//! §2.12 bridge, and the op allowlist holds nothing Mac-only.

use std::path::{Path, PathBuf};

use vault_ffi::*;

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Every unmangled export in the FFI's and the engine's sources, in the
/// attribute's own line or the next (review VER-O13 / SEC-O1).
fn exported_in_source() -> Vec<String> {
    let mut files = Vec::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    sources(&root.join("src"), &mut files);
    sources(&root.join("../vault-engine/src"), &mut files);
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            if let Some(rest) = l.split("export_name = \"").nth(1) {
                out.push(rest.split('"').next().unwrap().to_string());
            } else if l.contains("no_mangle") && !l.trim_start().starts_with("//") {
                let sig = if l.contains("fn ") { l } else { lines[i + 1] };
                out.push(sig.split("fn ").nth(1).unwrap().split('(').next().unwrap().to_string());
            }
        }
    }
    out.sort();
    out
}

fn sorted(list: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    v.sort();
    v
}

#[test]
fn the_sources_export_exactly_the_catalogue() {
    assert_eq!(exported_in_source(), sorted(CATALOGUE));
}

/// `nm` of the iOS static library: among `ov0_` symbols, the defined ones are exactly the
/// catalogue, imported ones exactly the bridge. The library must exist —
/// build it first (`cargo build -p vault-ffi --target aarch64-apple-ios`;
/// the F.2 gate does).
#[test]
fn the_ios_library_exports_the_catalogue_and_imports_the_bridge() {
    let lib = std::env::var("VAULT_FFI_IOS_LIB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/aarch64-apple-ios/debug/libvault_ffi.a"));
    assert!(lib.is_file(), "missing {} — build vault-ffi for aarch64-apple-ios first", lib.display());
    // The toolchain's own llvm-nm (`rustup component add llvm-tools`):
    // Xcode's nm is older than rustc's LLVM and cannot read std's objects.
    let sysroot = std::process::Command::new("rustc").args(["--print", "sysroot"]).output().expect("rustc");
    let sysroot = String::from_utf8(sysroot.stdout).unwrap();
    let nm = Path::new(sysroot.trim()).join("lib/rustlib/aarch64-apple-darwin/bin/llvm-nm");
    assert!(nm.is_file(), "missing {} — rustup component add llvm-tools", nm.display());
    let symbols = |flags: &[&str]| -> Vec<String> {
        let out = std::process::Command::new(&nm).args(flags).arg("-j").arg(&lib).output().expect("llvm-nm");
        assert!(out.status.success(), "llvm-nm {flags:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        let mut v: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().filter_map(|s| s.strip_prefix("_ov0_")).map(|s| format!("ov0_{s}")).collect();
        v.sort();
        v.dedup();
        v
    };
    assert_eq!(symbols(&["--defined-only", "-g"]), sorted(CATALOGUE), "defined");
    assert_eq!(symbols(&["-u"]), sorted(BRIDGE_IMPORTS), "imported");
}

/// Everything the engine dispatches that a phone must not run in F.2b.
pub const NOT_ON_THE_PHONE: &[&str] = &[
    "setup_vault", "peer_serve", "peer_serve_begin", "begin_enrollment", "enroll_hello", "enroll_confirm", "enroll_ack",
    "cancel_enrollment", "revoke_device", "recovery_begin", "recovery_preview", "recovery_complete", "setup_retry_handle",
    "rotate_recovery_key", "enroll_proof",
];

#[test]
fn the_op_allowlist_holds_nothing_mac_only() {
    for op in NOT_ON_THE_PHONE {
        assert!(!IOS_OPS.contains(op), "{op} is not an F.2b phone op");
    }
}

/// The phone's own ops are reachable through `ov0_engine_call` (review
/// VER-I1: a dropped entry would silently disable pairing or sync).
#[test]
fn the_op_allowlist_holds_the_phone_ops() {
    for op in ["join_begin", "join_hello", "join_bundle_begin", "join_complete", "join_finish", "join_abort", "peer_sync_begin", "peer_sync_step", "peer_sync_receive"] {
        assert!(IOS_OPS.contains(&op), "{op} is a phone op");
    }
}
