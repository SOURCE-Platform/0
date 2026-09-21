//! The helper links the §2.12 Swift bridge, whose Swift-runtime dylibs
//! carry `@rpath` install names. `cargo:rustc-link-arg` does not
//! propagate from a dependency's build script, so every crate that
//! produces a final binary against `vault_helper` has to add the runtime
//! search path itself — here, for the fuzz targets.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
