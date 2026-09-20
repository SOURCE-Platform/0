//! The `recovery-finalize` request body (spec §11.8), canonical TLV.

use sha2::{Digest, Sha256};

use crate::crypto::tlv::{EntryBuilder, EntryReader};
use crate::errors::ErrorCode;

mod tag {
    pub const PROTO: u8 = 0x01;
    pub const VAULT_ID: u8 = 0x02;
    pub const EXPECTED_OLD_GENERATION: u8 = 0x03;
    pub const EXPECTED_OLD_MANIFEST_HASH: u8 = 0x04;
    pub const EXPECTED_OLD_REGISTRY_HEAD: u8 = 0x05;
    pub const RECOVERY_CREDENTIAL_CLASS: u8 = 0x06;
    pub const RECOVERY_EPOCH_ENTRY: u8 = 0x07;
    pub const NEW_MANIFEST: u8 = 0x08;
    pub const NEW_REGISTRY_HEAD: u8 = 0x09;
    pub const NEW_VK_GENERATION: u8 = 0x0A;
    pub const NEW_DEVICE_BACKUP_CREDENTIAL: u8 = 0x0B;
    /// §4.8 registry checkpoint for the state being installed.
    pub const NEW_CHECKPOINT: u8 = 0x0C;
}

pub const CLASS_MP: u8 = 1;
pub const CLASS_RK: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalizeBody {
    pub vault_id: [u8; 16],
    pub expected_old_generation: u64,
    pub expected_old_manifest_hash: [u8; 32],
    pub expected_old_registry_head: [u8; 32],
    pub recovery_credential_class: u8,
    pub recovery_epoch_entry: Vec<u8>,
    pub new_manifest: Vec<u8>,
    pub new_registry_head: [u8; 32],
    pub new_vk_generation: u32,
    pub new_device_backup_credential: [u8; 32],
    pub new_checkpoint: Vec<u8>,
}

impl FinalizeBody {
    pub fn encode(&self) -> Vec<u8> {
        EntryBuilder::new()
            .field_uint(tag::PROTO, 1)
            .and_then(|b| b.field_bytes(tag::VAULT_ID, &self.vault_id))
            .and_then(|b| b.field_uint(tag::EXPECTED_OLD_GENERATION, self.expected_old_generation))
            .and_then(|b| b.field_bytes(tag::EXPECTED_OLD_MANIFEST_HASH, &self.expected_old_manifest_hash))
            .and_then(|b| b.field_bytes(tag::EXPECTED_OLD_REGISTRY_HEAD, &self.expected_old_registry_head))
            .and_then(|b| b.field_uint(tag::RECOVERY_CREDENTIAL_CLASS, u64::from(self.recovery_credential_class)))
            .and_then(|b| b.field_bytes(tag::RECOVERY_EPOCH_ENTRY, &self.recovery_epoch_entry))
            .and_then(|b| b.field_bytes(tag::NEW_MANIFEST, &self.new_manifest))
            .and_then(|b| b.field_bytes(tag::NEW_REGISTRY_HEAD, &self.new_registry_head))
            .and_then(|b| b.field_uint(tag::NEW_VK_GENERATION, u64::from(self.new_vk_generation)))
            .and_then(|b| b.field_bytes(tag::NEW_DEVICE_BACKUP_CREDENTIAL, &self.new_device_backup_credential))
            .and_then(|b| b.field_bytes(tag::NEW_CHECKPOINT, &self.new_checkpoint))
            .expect("ascending fields")
            .build()
    }

    pub fn sha256(&self) -> [u8; 32] {
        Sha256::digest(self.encode()).into()
    }

    pub fn decode(bytes: &[u8]) -> Result<FinalizeBody, ErrorCode> {
        let bad = |_| ErrorCode::InvalidInput;
        let r = EntryReader::parse(bytes).map_err(bad)?;
        if r.get_uint(tag::PROTO).map_err(bad)? != Some(1) {
            return Err(ErrorCode::InvalidInput);
        }
        let get = |t: u8| r.get(t).ok_or(ErrorCode::InvalidInput);
        let uint = |t: u8| r.get_uint(t).map_err(bad)?.ok_or(ErrorCode::InvalidInput);
        let arr16 = |t: u8| -> Result<[u8; 16], ErrorCode> { get(t)?.try_into().map_err(|_| ErrorCode::InvalidInput) };
        let arr32 = |t: u8| -> Result<[u8; 32], ErrorCode> { get(t)?.try_into().map_err(|_| ErrorCode::InvalidInput) };
        let body = FinalizeBody {
            vault_id: arr16(tag::VAULT_ID)?,
            expected_old_generation: uint(tag::EXPECTED_OLD_GENERATION)?,
            expected_old_manifest_hash: arr32(tag::EXPECTED_OLD_MANIFEST_HASH)?,
            expected_old_registry_head: arr32(tag::EXPECTED_OLD_REGISTRY_HEAD)?,
            recovery_credential_class: u8::try_from(uint(tag::RECOVERY_CREDENTIAL_CLASS)?).map_err(|_| ErrorCode::InvalidInput)?,
            recovery_epoch_entry: get(tag::RECOVERY_EPOCH_ENTRY)?.to_vec(),
            new_manifest: get(tag::NEW_MANIFEST)?.to_vec(),
            new_registry_head: arr32(tag::NEW_REGISTRY_HEAD)?,
            new_vk_generation: u32::try_from(uint(tag::NEW_VK_GENERATION)?).map_err(|_| ErrorCode::InvalidInput)?,
            new_device_backup_credential: arr32(tag::NEW_DEVICE_BACKUP_CREDENTIAL)?,
            new_checkpoint: get(tag::NEW_CHECKPOINT)?.to_vec(),
        };
        if body.encode() != bytes {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(body)
    }
}
