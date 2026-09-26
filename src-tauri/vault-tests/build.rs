//! The helper links the in-house Swift bridge (§2.12); test binaries that
//! link the helper need the system Swift runtime on their rpath, exactly
//! as the helper's own binaries do (see vault-helper/build.rs).
fn main() {
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}
