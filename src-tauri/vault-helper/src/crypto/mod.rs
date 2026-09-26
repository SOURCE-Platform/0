//! Phase B cryptographic core (spec §2, §4.2, §16.1). Library-only code:
//! no IPC surface changes, no storage engine, no network. Every secret
//! lives in a zeroizing type (§2.11); no secret type implements `Debug`.

pub mod bip39;
pub mod kdf;
pub mod record;
pub mod rotate;
pub mod vectors;
pub mod wrap;

// Moved to `vault-proto` (spec v0.4 §1.1); re-exported at their old paths.
pub use vault_proto::crypto::{ecdsa, hex, hkdf, recovery_auth, registry, secret, tlv, CryptoError};
