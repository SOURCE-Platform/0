//! Hand-maintained FFI surface. Every `unsafe` block here gets the
//! focused review required by spec §17.4 for crypto/unsafe code.

#[cfg(target_os = "macos")]
pub mod security;
