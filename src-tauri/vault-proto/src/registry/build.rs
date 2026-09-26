//! Building registry entries (§4.3). Signable kinds are signed by the
//! authorizing device's identity; the recovery_epoch entry (§4.5) carries
//! a proof keyed by the recovered VK instead of a signature and itself
//! installs the replacement device (rule 6).

use super::chain::RegistryState;
use super::device::DeviceIdentity;
use crate::crypto::registry::{self, EntryKind, RegistryEntry};
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;

fn now_epoch() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn device_entry(st: &RegistryState, kind: EntryKind, dev: &dyn DeviceIdentity) -> RegistryEntry {
    RegistryEntry {
        seq: st.next_seq(),
        prev_hash: st.head,
        epoch: st.epoch,
        kind,
        device_id: dev.device_id(),
        device_name: Some(dev.device_name()),
        platform: Some(dev.platform()),
        sign_pub: Some(dev.sign_pub()),
        agree_pub: Some(dev.agree_pub()),
        enrolled_at: Some(now_epoch()),
        authorizer: None,
        revoked_at: None,
        recovery_proof: None,
        manifest_hash: None,
        signature: None,
        prior_epoch: None,
        vault_id: None,
        recovery_nonce: None,
    }
}

fn sign(mut e: RegistryEntry, signer: &dyn DeviceIdentity) -> Result<RegistryEntry, ErrorCode> {
    let digest = registry::sign_input(&e).map_err(|_| ErrorCode::Internal)?;
    e.signature = Some(signer.sign_prehash(&digest).map_err(|_| ErrorCode::Internal)?);
    Ok(e)
}

/// seq 0: the first device, self-authorized and self-signed (rule 4).
pub fn genesis(dev: &dyn DeviceIdentity) -> Result<RegistryEntry, ErrorCode> {
    let st = RegistryState::empty();
    let mut e = device_entry(&st, EntryKind::Genesis, dev);
    e.authorizer = Some(dev.device_id());
    sign(e, dev)
}

/// `authorizer` enrolls `new_dev` (public identity only is used).
pub fn enroll(
    st: &RegistryState,
    authorizer: &dyn DeviceIdentity,
    new_dev: &dyn DeviceIdentity,
) -> Result<RegistryEntry, ErrorCode> {
    let mut e = device_entry(st, EntryKind::Enroll, new_dev);
    e.authorizer = Some(authorizer.device_id());
    sign(e, authorizer)
}

pub fn revoke(
    st: &RegistryState,
    authorizer: &dyn DeviceIdentity,
    target: [u8; 16],
) -> Result<RegistryEntry, ErrorCode> {
    let e = RegistryEntry {
        seq: st.next_seq(),
        prev_hash: st.head,
        epoch: st.epoch,
        kind: EntryKind::Revoke,
        device_id: target,
        device_name: None,
        platform: None,
        sign_pub: None,
        agree_pub: None,
        enrolled_at: None,
        authorizer: Some(authorizer.device_id()),
        revoked_at: Some(now_epoch()),
        recovery_proof: None,
        manifest_hash: None,
        signature: None,
        prior_epoch: None,
        vault_id: None,
        recovery_nonce: None,
    };
    sign(e, authorizer)
}

/// §4.5: the recovering device's epoch transition. `recovered_vk` is the
/// VK that protects `manifest_hash` (the state being recovered from);
/// the proof binds vault_id, prev_hash, manifest_hash, prior/new epoch,
/// the new device's id, keys, name, platform, and a fresh nonce.
pub fn recovery_epoch(
    st: &RegistryState,
    vault_id: [u8; 16],
    manifest_hash: [u8; 32],
    recovered_vk: &SecretBytes<32>,
    new_dev: &dyn DeviceIdentity,
) -> Result<RegistryEntry, ErrorCode> {
    let mut e = device_entry(st, EntryKind::RecoveryEpoch, new_dev);
    e.prior_epoch = Some(st.epoch);
    e.epoch = st.epoch + 1;
    e.manifest_hash = Some(manifest_hash);
    e.vault_id = Some(vault_id);
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).expect("OS RNG");
    e.recovery_nonce = Some(nonce);
    e.recovery_proof = Some(
        registry::compute_recovery_proof(recovered_vk, &manifest_hash, &e)
            .map_err(|_| ErrorCode::Internal)?,
    );
    Ok(e)
}
