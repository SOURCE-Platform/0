//! Enrollment frames as they cross the relay (§5.1). The main process
//! carries these between the TLS socket and the helper socket without
//! interpreting them; every field is either public or a value the helper
//! itself minted.

use serde::{Deserialize, Serialize};

/// Protocol version carried in the QR payload and ENROLL_HELLO. `v:2`
/// distinguishes vault enrollment from the legacy Source pairing payload.
pub const PROTO_V2: u8 = 2;

/// What the phone sends first (§5.2 ENROLL_HELLO). `device_id` is absent
/// on purpose: the Mac assigns it (see `HelloReply`), so a new device
/// cannot choose its own registry identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub proto: u8,
    /// Base32, repo alphabet, as printed in the QR.
    pub secret: String,
    pub nonce_n: String,
    pub sign_pub: String,
    pub agree_pub: String,
    pub name: String,
    pub platform: u8,
}

/// The Mac's answer: enough for the phone to compute the same transcript
/// (and therefore the same SAS) independently. The SAS itself is *not*
/// sent — each side derives it, which is what makes comparing the two
/// screens meaningful.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloReply {
    pub new_device_id: String,
    pub nonce_e: String,
    pub mac_device_id: String,
    pub vault_id: String,
}

/// Everything the phone needs to become a working device (§5.2 "Initial
/// vault transfer"). It is exactly a §11.2 snapshot — the same objects,
/// index, signed manifest and §4.8 checkpoint a backup publishes —
/// delivered over the enrollment channel instead of through a provider,
/// plus the HPKE envelope addressed to this device.
///
/// All ciphertext: the envelope is sealed to the phone's Secure Enclave
/// agreement key, the records are sealed under the VK, and the manifest
/// and registry are public verification state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    pub vault_id: String,
    /// `(object key, hex ciphertext)` — §3.7 objects, header, registry
    /// and wraps.
    pub objects: Vec<(String, String)>,
    /// Hex TLV of the signed manifest (§11.2).
    pub manifest: String,
    /// Hex TLV of the §4.8 registry checkpoint.
    pub checkpoint: String,
    pub envelope: serde_json::Value,
    /// The registry head the phone must ACK.
    pub registry_head: String,
}

/// The phone's signature over `SHA-256("ov0/enroll/ack/v1" ‖ head ‖ mac)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ack {
    pub signature: String,
}
