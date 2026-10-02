//! Production device identity (§2.7): two Secure-Enclave P-256 keys per
//! device — one for signing registry entries and enrollment ACKs, one for
//! HPKE agreement (device envelopes, §2.2/§2.9). Neither private key can
//! leave the Enclave; the helper holds only the 65-byte public keys.
//!
//! `identity.rs` is the `DeviceIdentity` implementation and its on-disk
//! public record, `envelope.rs` seals/opens `devices/<id>.wrap`, and
//! `se.rs` is the thin FFI over the §2.12 CryptoKit bridge.

pub mod envelope;
pub mod identity;
pub mod rotate;
pub mod se;

pub use envelope::{seal_envelope, open_envelope, DeviceEnvelopeFile, ENVELOPE_INFO_PREFIX};
pub use rotate::EnvelopePlan;
pub use identity::{SeDevice, DeviceFile, DEVICE_FILE_NAME};
