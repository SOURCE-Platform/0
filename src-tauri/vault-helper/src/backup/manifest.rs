//! `SignedManifest` (spec §11.2): canonical TLV, signed by the publishing
//! device over SHA-256("ov0/manifest/sign/v1" ‖ tlv(without 0x10)).
//! `manifest_hash` = SHA-256 of the full signed TLV bytes — the value a
//! recovery_epoch binds (§4.5) and finalize CASes against (§11.8).

use sha2::{Digest, Sha256};

use crate::crypto::ecdsa;
use crate::crypto::tlv::{EntryBuilder, EntryReader};
use crate::errors::ErrorCode;
use crate::registry::device::DeviceIdentity;

mod tag {
    pub const VERSION: u8 = 0x01;
    pub const VAULT_ID: u8 = 0x02;
    pub const GENERATION: u8 = 0x03;
    pub const CREATED_AT: u8 = 0x04;
    pub const REGISTRY_HEAD: u8 = 0x05;
    pub const VK_GENERATION: u8 = 0x06;
    pub const OBJECT_INDEX_HASH: u8 = 0x07;
    pub const PREV_MANIFEST_HASH: u8 = 0x08;
    pub const SIGNER_DEVICE_ID: u8 = 0x09;
    pub const SIGNATURE: u8 = 0x10;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedManifest {
    pub vault_id: [u8; 16],
    pub generation: u64,
    pub created_at: u64,
    pub registry_head: [u8; 32],
    pub vk_generation: u32,
    pub object_index_hash: [u8; 32],
    pub prev_manifest_hash: [u8; 32],
    pub signer_device_id: [u8; 16],
    pub signature: [u8; 64],
}

impl SignedManifest {
    fn tlv(&self, with_sig: bool) -> Vec<u8> {
        let mut b = EntryBuilder::new()
            .field_uint(tag::VERSION, 1)
            .and_then(|b| b.field_bytes(tag::VAULT_ID, &self.vault_id))
            .and_then(|b| b.field_uint(tag::GENERATION, self.generation))
            .and_then(|b| b.field_uint(tag::CREATED_AT, self.created_at))
            .and_then(|b| b.field_bytes(tag::REGISTRY_HEAD, &self.registry_head))
            .and_then(|b| b.field_uint(tag::VK_GENERATION, u64::from(self.vk_generation)))
            .and_then(|b| b.field_bytes(tag::OBJECT_INDEX_HASH, &self.object_index_hash))
            .and_then(|b| b.field_bytes(tag::PREV_MANIFEST_HASH, &self.prev_manifest_hash))
            .and_then(|b| b.field_bytes(tag::SIGNER_DEVICE_ID, &self.signer_device_id))
            .expect("ascending fixed-width fields");
        if with_sig {
            b = b.field_bytes(tag::SIGNATURE, &self.signature).expect("ascending");
        }
        b.build()
    }

    pub fn encode(&self) -> Vec<u8> {
        self.tlv(true)
    }

    pub fn sign_input(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b"ov0/manifest/sign/v1");
        h.update(self.tlv(false));
        h.finalize().into()
    }

    pub fn hash(&self) -> [u8; 32] {
        Sha256::digest(self.encode()).into()
    }

    /// Fill in signer + signature.
    pub fn sign(mut self, dev: &dyn DeviceIdentity) -> Result<Self, ErrorCode> {
        self.signer_device_id = dev.device_id();
        self.signature = [0u8; 64];
        self.signature = dev.sign_prehash(&self.sign_input()).map_err(|_| ErrorCode::Internal)?;
        Ok(self)
    }

    pub fn verify(&self, sign_pub: &[u8; 65]) -> Result<(), ErrorCode> {
        ecdsa::verify_prehash(sign_pub, &self.sign_input(), &self.signature)
            .map_err(|_| ErrorCode::SignatureInvalid)
    }

    /// Strict decode: canonical TLV, version 1, every field present with
    /// its exact width; re-encoding must reproduce the input.
    pub fn decode(bytes: &[u8]) -> Result<SignedManifest, ErrorCode> {
        let bad = |_| ErrorCode::ManifestMismatch;
        let r = EntryReader::parse(bytes).map_err(bad)?;
        if r.get_uint(tag::VERSION).map_err(bad)? != Some(1) {
            return Err(ErrorCode::FormatTooNew);
        }
        let fixed = |t: u8| r.get(t).ok_or(ErrorCode::ManifestMismatch);
        let uint = |t: u8| r.get_uint(t).map_err(bad)?.ok_or(ErrorCode::ManifestMismatch);
        let m = SignedManifest {
            vault_id: fixed(tag::VAULT_ID)?.try_into().map_err(|_| ErrorCode::ManifestMismatch)?,
            generation: uint(tag::GENERATION)?,
            created_at: uint(tag::CREATED_AT)?,
            registry_head: fixed(tag::REGISTRY_HEAD)?.try_into().map_err(|_| ErrorCode::ManifestMismatch)?,
            vk_generation: u32::try_from(uint(tag::VK_GENERATION)?).map_err(|_| ErrorCode::ManifestMismatch)?,
            object_index_hash: fixed(tag::OBJECT_INDEX_HASH)?.try_into().map_err(|_| ErrorCode::ManifestMismatch)?,
            prev_manifest_hash: fixed(tag::PREV_MANIFEST_HASH)?.try_into().map_err(|_| ErrorCode::ManifestMismatch)?,
            signer_device_id: fixed(tag::SIGNER_DEVICE_ID)?.try_into().map_err(|_| ErrorCode::ManifestMismatch)?,
            signature: fixed(tag::SIGNATURE)?.try_into().map_err(|_| ErrorCode::ManifestMismatch)?,
        };
        if m.encode() != bytes {
            return Err(ErrorCode::ManifestMismatch);
        }
        Ok(m)
    }
}
