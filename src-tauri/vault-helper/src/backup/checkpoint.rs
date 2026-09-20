//! Registry checkpoint (spec §4.8, Phase D.1): a MAC under the **current**
//! VK that binds the current registry head to the current manifest state.
//!
//! ```text
//! checkpoint_key = HKDF-SHA256(current_VK, salt=vault_id,
//!                              info="ov0/registry-checkpoint/v1")
//! registry_checkpoint = HMAC-SHA256(checkpoint_key,
//!     "ov0/registry-checkpoint/v1" ‖ tlv(version, vault_id, epoch,
//!         registry_head, manifest_core_hash, manifest_generation,
//!         vk_generation))
//! ```
//!
//! It lets a fresh recovery device anchor on the *current* registry after
//! recovering the current VK through MP or RK, instead of needing the
//! extinct VKs that keyed historical `recovery_epoch` proofs. No old VK or
//! old proof key is ever persisted.
//!
//! No recursion: `manifest_core_hash` covers the signed manifest's fields
//! **without** its signature, and the checkpoint is its own object that the
//! manifest and the object index never reference.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use super::manifest::SignedManifest;
use crate::crypto::hkdf::hkdf32;
use crate::crypto::secret::SecretBytes;
use crate::crypto::tlv::{EntryBuilder, EntryReader};
use crate::errors::ErrorCode;

pub const CHECKPOINT_VERSION: u32 = 1;
const INFO: &[u8] = b"ov0/registry-checkpoint/v1";

mod tag {
    pub const VERSION: u8 = 0x01;
    pub const VAULT_ID: u8 = 0x02;
    pub const EPOCH: u8 = 0x03;
    pub const REGISTRY_HEAD: u8 = 0x04;
    pub const MANIFEST_CORE_HASH: u8 = 0x05;
    pub const MANIFEST_GENERATION: u8 = 0x06;
    pub const VK_GENERATION: u8 = 0x07;
    pub const MAC: u8 = 0x08;
}

/// The bound state. Every field must be regenerated whenever it changes:
/// recovery epoch, any registry mutation, VK rotation, or a manifest
/// change covered by `manifest_core_hash`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryCheckpoint {
    pub vault_id: [u8; 16],
    pub epoch: u64,
    pub registry_head: [u8; 32],
    pub manifest_core_hash: [u8; 32],
    pub manifest_generation: u64,
    pub vk_generation: u32,
    pub mac: [u8; 32],
}

/// Canonical TLV. `with_mac=false` is the MAC input (§4.8).
fn tlv(c: &RegistryCheckpoint, with_mac: bool) -> Vec<u8> {
    let mut b = EntryBuilder::new()
        .field_uint(tag::VERSION, u64::from(CHECKPOINT_VERSION))
        .and_then(|b| b.field_bytes(tag::VAULT_ID, &c.vault_id))
        .and_then(|b| b.field_uint(tag::EPOCH, c.epoch))
        .and_then(|b| b.field_bytes(tag::REGISTRY_HEAD, &c.registry_head))
        .and_then(|b| b.field_bytes(tag::MANIFEST_CORE_HASH, &c.manifest_core_hash))
        .and_then(|b| b.field_uint(tag::MANIFEST_GENERATION, c.manifest_generation))
        .and_then(|b| b.field_uint(tag::VK_GENERATION, u64::from(c.vk_generation)))
        .expect("ascending fixed fields");
    if with_mac {
        b = b.field_bytes(tag::MAC, &c.mac).expect("ascending");
    }
    b.build()
}

pub fn checkpoint_key(vk: &SecretBytes<32>, vault_id: &[u8; 16]) -> Result<SecretBytes<32>, ErrorCode> {
    hkdf32(vk.expose(), vault_id, INFO).map_err(|_| ErrorCode::Internal)
}

fn mac_of(vk: &SecretBytes<32>, c: &RegistryCheckpoint) -> Result<[u8; 32], ErrorCode> {
    let key = checkpoint_key(vk, &c.vault_id)?;
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key.expose()).map_err(|_| ErrorCode::Internal)?;
    mac.update(INFO);
    mac.update(&tlv(c, false));
    Ok(mac.finalize().into_bytes().into())
}

impl RegistryCheckpoint {
    /// Build and MAC a checkpoint for `manifest` at `epoch` under `vk`.
    /// `manifest.registry_head` is the head it binds.
    pub fn create(vk: &SecretBytes<32>, manifest: &SignedManifest, epoch: u64) -> Result<RegistryCheckpoint, ErrorCode> {
        let mut c = RegistryCheckpoint {
            vault_id: manifest.vault_id,
            epoch,
            registry_head: manifest.registry_head,
            manifest_core_hash: manifest.core_hash(),
            manifest_generation: manifest.generation,
            vk_generation: manifest.vk_generation,
            mac: [0u8; 32],
        };
        c.mac = mac_of(vk, &c)?;
        Ok(c)
    }

    /// Verify the MAC under `vk` (the current VK, recovered via MP/RK).
    pub fn verify(&self, vk: &SecretBytes<32>) -> Result<(), ErrorCode> {
        let expected = mac_of(vk, self)?;
        if expected.ct_eq(&self.mac).into() {
            Ok(())
        } else {
            Err(ErrorCode::SignatureInvalid)
        }
    }

    /// Verify the MAC **and** that it binds exactly this manifest, this
    /// registry head, and this epoch (§4.8 step 3).
    pub fn verify_binding(
        &self,
        vk: &SecretBytes<32>,
        manifest: &SignedManifest,
        registry_head: &[u8; 32],
        epoch: u64,
    ) -> Result<(), ErrorCode> {
        self.verify(vk)?;
        let bound = self.vault_id == manifest.vault_id
            && self.registry_head == *registry_head
            && self.registry_head == manifest.registry_head
            && self.manifest_core_hash == manifest.core_hash()
            && self.manifest_generation == manifest.generation
            && self.vk_generation == manifest.vk_generation
            && self.epoch == epoch;
        if bound {
            Ok(())
        } else {
            Err(ErrorCode::ManifestMismatch)
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        tlv(self, true)
    }

    pub fn decode(bytes: &[u8]) -> Result<RegistryCheckpoint, ErrorCode> {
        let bad = |_| ErrorCode::ManifestMismatch;
        let r = EntryReader::parse(bytes).map_err(bad)?;
        if r.get_uint(tag::VERSION).map_err(bad)? != Some(u64::from(CHECKPOINT_VERSION)) {
            return Err(ErrorCode::FormatTooNew);
        }
        let fixed = |t: u8| r.get(t).ok_or(ErrorCode::ManifestMismatch);
        let uint = |t: u8| r.get_uint(t).map_err(bad)?.ok_or(ErrorCode::ManifestMismatch);
        let c = RegistryCheckpoint {
            vault_id: fixed(tag::VAULT_ID)?.try_into().map_err(|_| ErrorCode::ManifestMismatch)?,
            epoch: uint(tag::EPOCH)?,
            registry_head: fixed(tag::REGISTRY_HEAD)?.try_into().map_err(|_| ErrorCode::ManifestMismatch)?,
            manifest_core_hash: fixed(tag::MANIFEST_CORE_HASH)?.try_into().map_err(|_| ErrorCode::ManifestMismatch)?,
            manifest_generation: uint(tag::MANIFEST_GENERATION)?,
            vk_generation: u32::try_from(uint(tag::VK_GENERATION)?).map_err(|_| ErrorCode::ManifestMismatch)?,
            mac: fixed(tag::MAC)?.try_into().map_err(|_| ErrorCode::ManifestMismatch)?,
        };
        if c.encode() != bytes {
            return Err(ErrorCode::ManifestMismatch); // canonical form only
        }
        Ok(c)
    }

    /// Provider-side object key (never referenced by the object index, so
    /// the manifest/checkpoint hashes cannot become recursive).
    pub fn key(generation: u64) -> String {
        format!("objects/checkpoint/{generation}")
    }
}
