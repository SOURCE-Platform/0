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
    // PoC-only Swift surfaces (kept out of the production bridge).
    let shim = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("swift/PocShim.swift");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let status = Command::new("swiftc")
        .args(["-O", "-parse-as-library", "-emit-library", "-static", "-o"])
        .arg(out.join("libov0pocshim.a"))
        .arg(&shim)
        .status()
        .expect("swiftc shim");
    assert!(status.success(), "shim build failed");
    println!("cargo:rerun-if-changed={}", shim.display());
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=ov0pocshim");
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
