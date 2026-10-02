//! The §2.12 Apple bridge is built and linked by `vault-engine`'s build
//! script; a dependency's `rustc-link-arg` does not reach this crate's
//! binaries and tests, so the Swift runtime's search path is added here.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
