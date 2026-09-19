//! Registry chain verification (spec §4.4 rules 1–8, §4.5, §4.6).
//!
//! A registry is accepted only if every rule holds for every entry; the
//! first violation rejects the whole chain (callers never apply a
//! partially verified registry). Recovery-epoch entries (rule 6) need the
//! VK protecting the manifest they bind — the verifier supplies it through
//! `EpochContext`, together with its own view of which manifests are
//! acceptable (rollback / stale binding, RG-09, RG-16).

use crate::crypto::ecdsa;
use crate::crypto::registry::{self, EntryKind, RegistryEntry, ENTRY_VERSION_V2};
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;

/// One enrolled device as the verified chain describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRecord {
    pub device_id: [u8; 16],
    pub device_name: String,
    pub platform: u8,
    pub sign_pub: [u8; 65],
    pub agree_pub: [u8; 65],
    /// seq of the entry that installed it (genesis/enroll/recovery_epoch).
    pub installed_seq: u64,
    pub revoked: bool,
}

#[derive(Debug, Clone)]
pub struct RegistryState {
    pub entries: Vec<RegistryEntry>,
    pub devices: Vec<DeviceRecord>,
    pub epoch: u64,
    /// entry_hash of the last entry (32 zero bytes for an empty chain).
    pub head: [u8; 32],
}

impl RegistryState {
    pub fn empty() -> RegistryState {
        RegistryState { entries: Vec::new(), devices: Vec::new(), epoch: 0, head: [0u8; 32] }
    }

    pub fn next_seq(&self) -> u64 {
        self.entries.len() as u64
    }

    pub fn active_device(&self, id: &[u8; 16]) -> Option<&DeviceRecord> {
        self.devices.iter().find(|d| &d.device_id == id && !d.revoked)
    }
}

/// What the verifier knows that the chain cannot prove by itself.
pub trait EpochContext {
    /// The VK that protected the manifest `manifest_hash` names, if this
    /// verifier holds it. Without it a recovery proof cannot be checked
    /// and the entry is rejected (never accepted on trust).
    fn vk_for_manifest(&self, manifest_hash: &[u8; 32]) -> Option<SecretBytes<32>>;
    /// Whether `manifest_hash` names a manifest this verifier can confirm
    /// and has not superseded (§4.6: a newer accepted manifest makes an
    /// epoch bound to an older one a rollback).
    fn manifest_acceptable(&self, manifest_hash: &[u8; 32]) -> bool;
}

/// Verify a full chain for `vault_id`.
pub fn verify_chain(
    entries: &[RegistryEntry],
    vault_id: &[u8; 16],
    ctx: &dyn EpochContext,
) -> Result<RegistryState, ErrorCode> {
    let mut st = RegistryState::empty();
    for entry in entries {
        apply(&mut st, entry, vault_id, ctx)?;
    }
    Ok(st)
}

/// Verify one more entry on top of an already verified state.
pub fn apply(
    st: &mut RegistryState,
    e: &RegistryEntry,
    vault_id: &[u8; 16],
    ctx: &dyn EpochContext,
) -> Result<(), ErrorCode> {
    e.validate_presence().map_err(|_| ErrorCode::SignatureInvalid)?;
    e.validate_pubkeys().map_err(|_| ErrorCode::SignatureInvalid)?; // rule 8
    // Rules 1–2: contiguous seq, hash-linked.
    if e.seq != st.next_seq() || e.prev_hash != st.head {
        return Err(ErrorCode::RegistryTruncated);
    }
    match e.kind {
        EntryKind::Genesis => genesis(st, e)?,
        EntryKind::Enroll => enroll(st, e)?,
        EntryKind::Revoke => revoke(st, e)?,
        EntryKind::RecoveryEpoch => recovery_epoch(st, e, vault_id, ctx)?,
    }
    st.head = registry::entry_hash(e).map_err(|_| ErrorCode::SignatureInvalid)?;
    st.entries.push(e.clone());
    Ok(())
}

fn verify_sig(e: &RegistryEntry, sign_pub: &[u8; 65]) -> Result<(), ErrorCode> {
    let digest = registry::sign_input(e).map_err(|_| ErrorCode::SignatureInvalid)?;
    let sig = e.signature.ok_or(ErrorCode::SignatureInvalid)?;
    ecdsa::verify_prehash(sign_pub, &digest, &sig).map_err(|_| ErrorCode::SignatureInvalid)
}

fn install(st: &mut RegistryState, e: &RegistryEntry) -> Result<(), ErrorCode> {
    // Rule 5 / rule 6: an identity is installed once, ever; reusing a
    // device_id (even a revoked one) never re-trusts it.
    if st.devices.iter().any(|d| d.device_id == e.device_id) {
        return Err(ErrorCode::DeviceNotAuthorized);
    }
    st.devices.push(DeviceRecord {
        device_id: e.device_id,
        device_name: e.device_name.clone().ok_or(ErrorCode::SignatureInvalid)?,
        platform: e.platform.ok_or(ErrorCode::SignatureInvalid)?,
        sign_pub: e.sign_pub.ok_or(ErrorCode::SignatureInvalid)?,
        agree_pub: e.agree_pub.ok_or(ErrorCode::SignatureInvalid)?,
        installed_seq: e.seq,
        revoked: false,
    });
    Ok(())
}

/// Rule 4: first entry only, self-authorized, self-signed, epoch 0.
fn genesis(st: &mut RegistryState, e: &RegistryEntry) -> Result<(), ErrorCode> {
    if e.seq != 0 || e.epoch != 0 || e.authorizer != Some(e.device_id) {
        return Err(ErrorCode::DeviceNotAuthorized);
    }
    verify_sig(e, &e.sign_pub.ok_or(ErrorCode::SignatureInvalid)?)?;
    install(st, e)
}

/// Rule 3: signed by an enrolled, non-revoked authorizer other than the
/// new device (RG-11: no self-signed enroll exists in v2).
fn enroll(st: &mut RegistryState, e: &RegistryEntry) -> Result<(), ErrorCode> {
    let auth = e.authorizer.ok_or(ErrorCode::DeviceNotAuthorized)?;
    if e.seq == 0 || auth == e.device_id || e.epoch != st.epoch {
        return Err(ErrorCode::DeviceNotAuthorized);
    }
    let signer = st.active_device(&auth).ok_or(ErrorCode::DeviceNotAuthorized)?.sign_pub;
    verify_sig(e, &signer)?;
    install(st, e)
}

/// Rules 3 + 5: signed by an active authorizer; target active; once.
fn revoke(st: &mut RegistryState, e: &RegistryEntry) -> Result<(), ErrorCode> {
    let auth = e.authorizer.ok_or(ErrorCode::DeviceNotAuthorized)?;
    if e.seq == 0 || e.epoch != st.epoch {
        return Err(ErrorCode::DeviceNotAuthorized);
    }
    let signer = st.active_device(&auth).ok_or(ErrorCode::DeviceNotAuthorized)?.sign_pub;
    verify_sig(e, &signer)?;
    let target = st
        .devices
        .iter_mut()
        .find(|d| d.device_id == e.device_id && !d.revoked)
        .ok_or(ErrorCode::DeviceNotAuthorized)?;
    target.revoked = true;
    Ok(())
}

/// Rule 6: proof-authorized epoch transition that installs exactly the
/// proof-bound replacement device.
fn recovery_epoch(
    st: &mut RegistryState,
    e: &RegistryEntry,
    vault_id: &[u8; 16],
    ctx: &dyn EpochContext,
) -> Result<(), ErrorCode> {
    if e.seq == 0 || e.vault_id.as_ref() != Some(vault_id) {
        return Err(ErrorCode::DeviceNotAuthorized);
    }
    let prior = e.prior_epoch.ok_or(ErrorCode::SignatureInvalid)?;
    if prior != st.epoch || e.epoch != prior + 1 {
        return Err(ErrorCode::DeviceNotAuthorized);
    }
    let manifest_hash = e.manifest_hash.ok_or(ErrorCode::SignatureInvalid)?;
    if !ctx.manifest_acceptable(&manifest_hash) {
        return Err(ErrorCode::ManifestRollback);
    }
    let vk = ctx.vk_for_manifest(&manifest_hash).ok_or(ErrorCode::DeviceNotAuthorized)?;
    registry::verify_recovery_proof(&vk, &manifest_hash, e).map_err(|_| ErrorCode::SignatureInvalid)?;
    install(st, e)?;
    st.epoch = e.epoch;
    Ok(())
}

/// §4.6: compare a downloaded chain with the locally accepted one.
/// Divergence at any shared seq → REGISTRY_FORK; shorter → TRUNCATED.
pub fn check_extends(local: &[RegistryEntry], remote: &[RegistryEntry]) -> Result<(), ErrorCode> {
    if remote.len() < local.len() {
        return Err(ErrorCode::RegistryTruncated);
    }
    for (a, b) in local.iter().zip(remote) {
        let ha = registry::entry_hash(a).map_err(|_| ErrorCode::SignatureInvalid)?;
        let hb = registry::entry_hash(b).map_err(|_| ErrorCode::SignatureInvalid)?;
        if ha != hb {
            return Err(ErrorCode::RegistryFork);
        }
    }
    Ok(())
}

/// Every entry must carry entry_version 2 (encode_tlv writes it; decode
/// rejects others) — re-exported so callers can assert the constant.
pub const REQUIRED_ENTRY_VERSION: u32 = ENTRY_VERSION_V2;
