//! Device registry entries (spec §4): the canonical TLV model used for
//! hashing, signing, and recovery proofs. Chain validation (§4.4) and
//! signature authorization are Phase E scope; Phase B ships the codec, the
//! presence rules, the hash/proof constructions, and the RG-10
//! canonicalization guarantees everything else builds on.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

use super::ecdsa;
use super::secret::SecretBytes;
use super::tlv::{EntryBuilder, EntryReader};
use super::CryptoError;

pub const ENTRY_VERSION_V2: u32 = 2;
pub const DEVICE_NAME_MAX_CHARS: usize = 64;

pub mod tag {
    pub const ENTRY_VERSION: u8 = 0x01;
    pub const SEQ: u8 = 0x02;
    pub const PREV_HASH: u8 = 0x03;
    pub const EPOCH: u8 = 0x04;
    pub const KIND: u8 = 0x05;
    pub const DEVICE_ID: u8 = 0x06;
    pub const DEVICE_NAME: u8 = 0x07;
    pub const PLATFORM: u8 = 0x08;
    pub const SIGN_PUB: u8 = 0x09;
    pub const AGREE_PUB: u8 = 0x0A;
    pub const ENROLLED_AT: u8 = 0x0B;
    pub const AUTHORIZER: u8 = 0x0C;
    pub const REVOKED_AT: u8 = 0x0D;
    pub const RECOVERY_PROOF: u8 = 0x0E;
    pub const MANIFEST_HASH: u8 = 0x0F;
    pub const SIGNATURE: u8 = 0x10;
    pub const PRIOR_EPOCH: u8 = 0x11;
    pub const VAULT_ID: u8 = 0x12;
    pub const RECOVERY_NONCE: u8 = 0x13;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Genesis = 1,
    Enroll = 2,
    Revoke = 3,
    RecoveryEpoch = 4,
}

impl EntryKind {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(EntryKind::Genesis),
            2 => Some(EntryKind::Enroll),
            3 => Some(EntryKind::Revoke),
            4 => Some(EntryKind::RecoveryEpoch),
            _ => None,
        }
    }
}

/// One registry entry. `Option` fields are kind-dependent; §4.3 presence
/// rules are enforced by both `encode_tlv` and `validate_presence`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntry {
    pub seq: u64,
    pub prev_hash: [u8; 32],
    pub epoch: u64,
    pub kind: EntryKind,
    pub device_id: [u8; 16],
    pub device_name: Option<String>,
    pub platform: Option<u8>, // 1 macos, 2 ios
    pub sign_pub: Option<[u8; 65]>,
    pub agree_pub: Option<[u8; 65]>,
    pub enrolled_at: Option<u64>,
    pub authorizer: Option<[u8; 16]>,
    pub revoked_at: Option<u64>,
    pub recovery_proof: Option<[u8; 32]>,
    pub manifest_hash: Option<[u8; 32]>,
    pub signature: Option<[u8; 64]>,
    pub prior_epoch: Option<u64>,
    pub vault_id: Option<[u8; 16]>,
    pub recovery_nonce: Option<[u8; 16]>,
}

#[allow(clippy::too_many_arguments)]
impl RegistryEntry {
    /// §4.3 presence rules. Returns Err(FieldPresence) on any violation.
    pub fn validate_presence(&self) -> Result<(), CryptoError> {
        fn has<T>(o: &Option<T>) -> bool {
            o.is_some()
        }
        let device_fields = has(&self.device_name)
            && has(&self.platform)
            && has(&self.sign_pub)
            && has(&self.agree_pub)
            && has(&self.enrolled_at);
        let recovery_fields = has(&self.recovery_proof)
            && has(&self.manifest_hash)
            && has(&self.prior_epoch)
            && has(&self.vault_id)
            && has(&self.recovery_nonce);
        let ok = match self.kind {
            EntryKind::Genesis | EntryKind::Enroll => {
                device_fields
                    && has(&self.authorizer)
                    && has(&self.signature)
                    && !has(&self.revoked_at)
                    && !recovery_fields
            }
            EntryKind::Revoke => {
                has(&self.authorizer)
                    && has(&self.revoked_at)
                    && has(&self.signature)
                    && !device_fields
                    && !recovery_fields
            }
            EntryKind::RecoveryEpoch => {
                device_fields
                    && recovery_fields
                    && !has(&self.authorizer)
                    && !has(&self.signature)
                    && !has(&self.revoked_at)
            }
        };
        if ok {
            Ok(())
        } else {
            Err(CryptoError::FieldPresence)
        }
    }

    /// On-curve / length check for public key fields (§4.4 rule 8).
    pub fn validate_pubkeys(&self) -> Result<(), CryptoError> {
        for key in [self.sign_pub, self.agree_pub].into_iter().flatten() {
            ecdsa::parse_verifying_key(&key)?;
        }
        Ok(())
    }

    /// Canonical TLV encoding (§4.2/§4.3). `with_proof_or_sig` controls
    /// whether the terminal authorization field (0x0E or 0x10) is
    /// included — sign_input and proof input both exclude it.
    pub fn encode_tlv(&self, with_terminal_field: bool) -> Result<Vec<u8>, CryptoError> {
        self.validate_presence()?;
        self.validate_pubkeys()?;
        let b = EntryBuilder::new()
            .field_uint(tag::ENTRY_VERSION, ENTRY_VERSION_V2 as u64)?
            .field_uint(tag::SEQ, self.seq)?
            .field_bytes(tag::PREV_HASH, &self.prev_hash)?
            .field_uint(tag::EPOCH, self.epoch)?
            .field_uint(tag::KIND, self.kind as u64)?
            .field_bytes(tag::DEVICE_ID, &self.device_id)?;
        let b = match self.kind {
            EntryKind::Genesis | EntryKind::Enroll | EntryKind::RecoveryEpoch => b
                .field_string(tag::DEVICE_NAME, self.device_name.as_deref().unwrap_or(""))?
                .field_uint(tag::PLATFORM, self.platform.unwrap_or(0) as u64)?
                .field_bytes(tag::SIGN_PUB, &self.sign_pub.unwrap_or([0; 65]))?
                .field_bytes(tag::AGREE_PUB, &self.agree_pub.unwrap_or([0; 65]))?
                .field_uint(tag::ENROLLED_AT, self.enrolled_at.unwrap_or(0))?,
            EntryKind::Revoke => b,
        };
        let b = match self.kind {
            EntryKind::Genesis | EntryKind::Enroll | EntryKind::Revoke => {
                b.field_bytes(tag::AUTHORIZER, &self.authorizer.unwrap_or([0; 16]))?
            }
            EntryKind::RecoveryEpoch => b,
        };
        let b = match self.kind {
            EntryKind::Revoke => b.field_uint(tag::REVOKED_AT, self.revoked_at.unwrap_or(0))?,
            _ => b,
        };
        let b = match self.kind {
            EntryKind::RecoveryEpoch => {
                let b = if with_terminal_field {
                    b.field_bytes(tag::RECOVERY_PROOF, &self.recovery_proof.unwrap_or([0; 32]))?
                } else {
                    b
                };
                b.field_bytes(tag::MANIFEST_HASH, &self.manifest_hash.unwrap_or([0; 32]))?
                    .field_uint(tag::PRIOR_EPOCH, self.prior_epoch.unwrap_or(0))?
                    .field_bytes(tag::VAULT_ID, &self.vault_id.unwrap_or([0; 16]))?
                    .field_bytes(tag::RECOVERY_NONCE, &self.recovery_nonce.unwrap_or([0; 16]))?
            }
            _ => b,
        };
        let b = match self.kind {
            EntryKind::Genesis | EntryKind::Enroll | EntryKind::Revoke if with_terminal_field => {
                b.field_bytes(tag::SIGNATURE, &self.signature.unwrap_or([0; 64]))?
            }
            _ => b,
        };
        Ok(b.build())
    }

    /// Strict decode + presence validation (§4.2 canonical rules apply
    /// before any semantic check — see RG-10).
    pub fn decode_tlv(bytes: &[u8]) -> Result<Self, CryptoError> {
        let r = EntryReader::parse(bytes)?;
        let fixed32 = |t: u8| -> Result<Option<[u8; 32]>, CryptoError> {
            r.get(t)
                .map(|b| <[u8; 32]>::try_from(b).map_err(|_| CryptoError::FieldPresence))
                .transpose()
        };
        let fixed16 = |t: u8| -> Result<Option<[u8; 16]>, CryptoError> {
            r.get(t)
                .map(|b| <[u8; 16]>::try_from(b).map_err(|_| CryptoError::FieldPresence))
                .transpose()
        };
        let version = r
            .get_uint(tag::ENTRY_VERSION)?
            .ok_or(CryptoError::FieldPresence)?;
        if version != ENTRY_VERSION_V2 as u64 {
            return Err(CryptoError::FieldPresence); // unknown version: refuse (§4.3)
        }
        let kind_byte = r.get_uint(tag::KIND)?.ok_or(CryptoError::FieldPresence)? as u8;
        let kind = EntryKind::from_u8(kind_byte).ok_or(CryptoError::FieldPresence)?;
        let signature = r
            .get(tag::SIGNATURE)
            .map(|b| <[u8; 64]>::try_from(b).map_err(|_| CryptoError::FieldPresence))
            .transpose()?;
        let entry = RegistryEntry {
            seq: r.get_uint(tag::SEQ)?.ok_or(CryptoError::FieldPresence)?,
            prev_hash: fixed32(tag::PREV_HASH)?.ok_or(CryptoError::FieldPresence)?,
            epoch: r.get_uint(tag::EPOCH)?.ok_or(CryptoError::FieldPresence)?,
            kind,
            device_id: fixed16(tag::DEVICE_ID)?.ok_or(CryptoError::FieldPresence)?,
            device_name: r.get_string(tag::DEVICE_NAME, DEVICE_NAME_MAX_CHARS)?,
            platform: r.get_uint(tag::PLATFORM)?.map(|v| v as u8),
            sign_pub: fixed65(&r, tag::SIGN_PUB)?,
            agree_pub: fixed65(&r, tag::AGREE_PUB)?,
            enrolled_at: r.get_uint(tag::ENROLLED_AT)?,
            authorizer: fixed16(tag::AUTHORIZER)?,
            revoked_at: r.get_uint(tag::REVOKED_AT)?,
            recovery_proof: fixed32(tag::RECOVERY_PROOF)?,
            manifest_hash: fixed32(tag::MANIFEST_HASH)?,
            signature,
            prior_epoch: r.get_uint(tag::PRIOR_EPOCH)?,
            vault_id: fixed16(tag::VAULT_ID)?,
            recovery_nonce: fixed16(tag::RECOVERY_NONCE)?,
        };
        entry.validate_presence()?;
        entry.validate_pubkeys()?;
        // Every decoded tag must be a known §4.3 tag.
        for t in r.tags() {
            if !(tag::ENTRY_VERSION..=tag::RECOVERY_NONCE).contains(&t) {
                return Err(CryptoError::FieldPresence);
            }
        }
        Ok(entry)
    }
}

fn fixed65(r: &EntryReader<'_>, t: u8) -> Result<Option<[u8; 65]>, CryptoError> {
    r.get(t)
        .map(|b| <[u8; 65]>::try_from(b).map_err(|_| CryptoError::FieldPresence))
        .transpose()
}

/// entry_hash = SHA-256("ov0/registry/entry/v1" ‖ tlv(entry)) — the
/// terminal authorization field included (§4.3).
pub fn entry_hash(entry: &RegistryEntry) -> Result<[u8; 32], CryptoError> {
    let tlv = entry.encode_tlv(true)?;
    let mut h = Sha256::new();
    h.update(b"ov0/registry/entry/v1");
    h.update(&tlv);
    Ok(h.finalize().into())
}

/// sign_input = SHA-256("ov0/registry/sign/v1" ‖ tlv(entry ∖ 0x10)).
/// The signature field cannot satisfy its own presence rule while it is
/// being produced, so signable kinds are validated with a placeholder
/// that is never encoded (0x10 is terminal and only emitted by
/// encode_tlv(true)). recovery_epoch is never signed (§4.4 rule 6).
pub fn sign_input(entry: &RegistryEntry) -> Result<[u8; 32], CryptoError> {
    let mut check = entry.clone();
    match check.kind {
        EntryKind::Genesis | EntryKind::Enroll | EntryKind::Revoke => {
            check.signature.get_or_insert([0u8; 64]);
        }
        EntryKind::RecoveryEpoch => return Err(CryptoError::FieldPresence),
    }
    let tlv = check.encode_tlv(false)?;
    let mut h = Sha256::new();
    h.update(b"ov0/registry/sign/v1");
    h.update(&tlv);
    Ok(h.finalize().into())
}

/// §4.5: recovery_proof = HMAC-SHA256(recovery_key, domain ‖ tlv(entry ∖
/// 0x0E)), recovery_key = HKDF(ikm=VK, salt=manifest_hash,
/// info="ov0/recovery-auth/v1"). Binds vault_id, prev_hash, manifest_hash,
/// prior/new epoch, the new device's id and public keys, and the fresh
/// recovery_nonce — any alteration fails verification (RG-08, RG-13…15).
pub fn compute_recovery_proof(
    vk: &SecretBytes<32>,
    manifest_hash: &[u8; 32],
    entry: &RegistryEntry,
) -> Result<[u8; 32], CryptoError> {
    let key = super::hkdf::recovery_auth_key(vk, manifest_hash)?;
    // As with sign_input: the proof field cannot satisfy its own
    // presence rule while being produced; validate with a placeholder
    // that encode_tlv(false) never emits.
    let mut check = entry.clone();
    if check.kind != EntryKind::RecoveryEpoch {
        return Err(CryptoError::FieldPresence);
    }
    check.recovery_proof.get_or_insert([0u8; 32]);
    let tlv = check.encode_tlv(false)?;
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key.expose())
        .expect("HMAC accepts any key length");
    mac.update(b"ov0/registry/recovery/v1");
    mac.update(&tlv);
    Ok(mac.finalize().into_bytes().into())
}

/// Constant-time-ish proof check (HMAC compare via recompute; the proof
/// is public verification state, §4.7, so timing is not load-bearing here,
/// but we still compare via `subtle`).
pub fn verify_recovery_proof(
    vk: &SecretBytes<32>,
    manifest_hash: &[u8; 32],
    entry: &RegistryEntry,
) -> Result<(), CryptoError> {
    let expected = compute_recovery_proof(vk, manifest_hash, entry)?;
    let presented = entry.recovery_proof.ok_or(CryptoError::FieldPresence)?;
    use subtle::ConstantTimeEq;
    if expected.ct_eq(&presented).into() {
        Ok(())
    } else {
        Err(CryptoError::SignatureInvalid)
    }
}
