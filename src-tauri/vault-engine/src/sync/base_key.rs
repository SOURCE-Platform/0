//! The base key of a pending local rotation (spec v0.5 §2.7; review SEC-I4
//! of e7da81a, closed in the F.2d chunk review SEC-B1 / VER-B2): while a
//! rotation of this device's own waits to publish, the VK of its base
//! generation is kept sealed under the current VK. A served state at the
//! base generation — another device's publication made meanwhile — then
//! opens with no envelope and no master password, so a pending revocation
//! never waits on a password the user may have reset. The key is trusted
//! only as far as the served checkpoint binding verifies under it.
//!
//! Sealing: XChaCha20-Poly1305 under `HKDF(VK, vault_id,
//! "ov0/pending-base-key/v1")`, AAD = info ‖ vault_id ‖ base generation ‖
//! sealing generation. Whoever holds the current VK learns nothing new:
//! every record under the base key was re-sealed under the current one.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::crypto::hex;
use crate::crypto::hkdf::hkdf32;
use crate::crypto::secret::{random_nonce, SecretBytes};
use crate::errors::ErrorCode;
use crate::storage::kv;

pub const KEY: &str = "pending_base_key";
const INFO: &[u8] = b"ov0/pending-base-key/v1";

#[derive(Serialize, Deserialize)]
struct Sealed {
    base_gen: u32,
    under_gen: u32,
    nonce: String,
    ct: String,
}

fn aad(vid: &[u8; 16], base_gen: u32, under_gen: u32) -> Vec<u8> {
    let mut a = INFO.to_vec();
    a.extend_from_slice(vid);
    a.extend_from_slice(&base_gen.to_be_bytes());
    a.extend_from_slice(&under_gen.to_be_bytes());
    a
}

fn cipher(vk: &SecretBytes<32>, vid: &[u8; 16]) -> Result<XChaCha20Poly1305, ErrorCode> {
    let k = hkdf32(vk.expose(), vid, INFO).map_err(|_| ErrorCode::Internal)?;
    XChaCha20Poly1305::new_from_slice(k.expose()).map_err(|_| ErrorCode::Internal)
}

/// The base key sealed under `vk` (generation `under_gen`), if it is the
/// key of `base_gen`. Anything else — absent, another generation, failing
/// authentication — is `None`: the caller falls back to the envelope.
pub fn open(conn: &Connection, vid: &[u8; 16], vk: &SecretBytes<32>, under_gen: u32, base_gen: u32) -> Result<Option<SecretBytes<32>>, ErrorCode> {
    let Some(s) = kv::get::<Sealed>(conn, KEY)? else { return Ok(None) };
    if s.base_gen != base_gen || s.under_gen != under_gen {
        return Ok(None);
    }
    let (Some(nonce), Some(ct)) = (hex::decode_array::<24>(&s.nonce), hex::decode(&s.ct)) else { return Ok(None) };
    let Ok(mut pt) = cipher(vk, vid)?.decrypt(&XNonce::from(nonce), Payload { msg: &ct, aad: &aad(vid, base_gen, under_gen) }) else {
        return Ok(None);
    };
    let out = <[u8; 32]>::try_from(pt.as_slice()).ok().map(SecretBytes::new);
    pt.zeroize();
    Ok(out)
}

/// Inside a rotation's staged database (old generation → `new_gen`): keep
/// the pending record's base key, sealed under the new VK. The base is the
/// old key when the record was built on it; otherwise it is the key the
/// previous rotation carried. With no pending record, nothing is kept.
pub fn carry(conn: &Connection, vid: &[u8; 16], old_vk: &SecretBytes<32>, old_gen: u32, new_vk: &SecretBytes<32>, new_gen: u32) -> Result<(), ErrorCode> {
    let Some(p) = super::pending::load(conn)? else { return kv::delete(conn, KEY) };
    let base_gen = p.base.vk_generation;
    let base = if base_gen == old_gen { Some(SecretBytes::new(*old_vk.expose())) } else { open(conn, vid, old_vk, old_gen, base_gen)? };
    let Some(base) = base else { return kv::delete(conn, KEY) };
    let nonce = random_nonce();
    let ct = cipher(new_vk, vid)?
        .encrypt(&XNonce::from(nonce), Payload { msg: base.expose(), aad: &aad(vid, base_gen, new_gen) })
        .map_err(|_| ErrorCode::Internal)?;
    kv::put(conn, KEY, &Sealed { base_gen, under_gen: new_gen, nonce: hex::encode(nonce), ct: hex::encode(ct) })
}
