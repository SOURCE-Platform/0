//! Build the §2.12 Swift bridge and link it into the PoC (static, C ABI).

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let pkg = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../vault-apple-crypto")
        .canonicalize()
        .expect("bridge package path");
    let status = Command::new("swift")
        .args(["build", "-c", "release", "--package-path"])
        .arg(&pkg)
        .status()
        .expect("swift build");
    assert!(status.success(), "swift build failed");
    let lib_dir = pkg.join(".build/release");
    println!("cargo:rerun-if-changed={}", pkg.join("Sources/VaultAppleCrypto/Bridge.swift").display());
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=VaultAppleCrypto");
    // Swift runtime + frameworks the bridge uses.
    println!("cargo:rustc-link-search=native=/usr/lib/swift");
    // The OS-provided Swift runtime lives outside the default search set.
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    for framework in ["CryptoKit", "Foundation", "Security"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
    for lib in ["swiftCore", "swiftFoundation", "swiftDarwin", "swiftObjectiveC", "swift_Concurrency", "swiftDispatch", "swiftCoreFoundation", "swiftIOKit", "swiftXPC"] {
        println!("cargo:rustc-link-lib=dylib={lib}");
    }
}
