//! Build and link the §2.12 Apple bridge (CryptoKit HPKE + Secure Enclave).
//! macOS only: the vault's device identity and envelopes are macOS/iOS
//! features (§2.12 floor: macOS 14+).

use std::path::PathBuf;
use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let pkg = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../vault-apple-crypto")
        .canonicalize()
        .expect("bridge package");
    let status = Command::new("swift")
        .args(["build", "-c", "release", "--package-path"])
        .arg(&pkg)
        .status()
        .expect("swift build (Xcode command line tools required)");
    assert!(status.success(), "swift build of vault-apple-crypto failed");
    println!("cargo:rerun-if-changed={}", pkg.join("Sources/VaultAppleCrypto/Bridge.swift").display());
    println!("cargo:rustc-link-search=native={}", pkg.join(".build/release").display());
    println!("cargo:rustc-link-lib=static=VaultAppleCrypto");
    println!("cargo:rustc-link-search=native=/usr/lib/swift");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    for framework in ["CryptoKit", "Foundation", "Security"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
    for lib in ["swiftCore", "swiftFoundation", "swiftDarwin", "swiftObjectiveC", "swift_Concurrency", "swiftDispatch", "swiftCoreFoundation", "swiftIOKit", "swiftXPC"] {
        println!("cargo:rustc-link-lib=dylib={lib}");
    }
}
