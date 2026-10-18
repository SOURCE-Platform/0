//! §11.3 step 6: the registry blob of a transition. The current registry
//! must be an exact prefix (by entry hash); the whole chain verifies under
//! the structural rules (§4.4 rules 1–5, 7, 8; recovery epochs structurally,
//! as in §4.8); and the appended entries obey the transition kind.

use vault_proto::crypto::registry::{entry_hash, EntryKind, RegistryEntry};
use vault_proto::errors::ErrorCode;
use vault_proto::registry::chain::{verify_chain_with, EpochPolicy, RegistryState};
use vault_proto::registry::file;
use vault_proto::state::TransitionKind;

use crate::model::VaultStateDoc;

pub struct RegistryOutcome {
    pub state: RegistryState,
    /// Device ids revoked by the appended entries.
    pub revoked: Vec<[u8; 16]>,
}

pub fn check(
    kind: TransitionKind,
    cur: Option<(&VaultStateDoc, &[RegistryEntry])>,
    bytes: &[u8],
    vault_id: &[u8; 16],
) -> Result<RegistryOutcome, ErrorCode> {
    let invalid = |_| ErrorCode::RegistryInvalid;
    let entries = file::decode(bytes).map_err(invalid)?;
    let prior: &[RegistryEntry] = cur.map_or(&[], |(_, e)| e);
    if entries.len() < prior.len() {
        return Err(ErrorCode::RegistryInvalid);
    }
    for (a, b) in prior.iter().zip(&entries) {
        if entry_hash(a).map_err(|_| ErrorCode::RegistryInvalid)? != entry_hash(b).map_err(|_| ErrorCode::RegistryInvalid)? {
            return Err(ErrorCode::RegistryInvalid);
        }
    }
    let state = verify_chain_with(&entries, vault_id, &EpochPolicy::CheckpointAnchored).map_err(invalid)?;
    let appended = &entries[prior.len()..];
    let revoked: Vec<[u8; 16]> = appended.iter().filter(|e| e.kind == EntryKind::Revoke).map(|e| e.device_id).collect();
    let ok = match (kind, cur) {
        (TransitionKind::Create, None) => entries.len() == 1 && entries[0].kind == EntryKind::Genesis,
        (TransitionKind::Publish, Some(_)) => {
            appended.iter().all(|e| matches!(e.kind, EntryKind::Enroll | EntryKind::Revoke))
        }
        (TransitionKind::Finalize, Some((cur, _))) => {
            let active: Vec<[u8; 16]> = cur.active_devices.iter().map(|d| d.device_id.0).collect();
            finalize_shape(cur.epoch, &active, appended)
        }
        _ => false,
    };
    if !ok {
        return Err(ErrorCode::RegistryInvalid);
    }
    Ok(RegistryOutcome { state, revoked })
}

/// Exactly one `recovery_epoch` (epoch = current + 1), then one `revoke`
/// per currently active device in ascending `device_id` order, each
/// authorized by the epoch's device (§4.4 S-4); nothing else. The chain
/// verifier already checked each revoke's signature under its authorizer.
fn finalize_shape(cur_epoch: u64, active: &[[u8; 16]], appended: &[RegistryEntry]) -> bool {
    let Some((epoch, revokes)) = appended.split_first() else {
        return false;
    };
    if epoch.kind != EntryKind::RecoveryEpoch || epoch.epoch != cur_epoch + 1 {
        return false;
    }
    let mut prior: Vec<[u8; 16]> = active.to_vec();
    prior.sort();
    revokes.len() == prior.len()
        && revokes.iter().zip(&prior).all(|(r, id)| {
            r.kind == EntryKind::Revoke && &r.device_id == id && r.authorizer == Some(epoch.device_id)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_proto::crypto::secret::SecretBytes;
    use vault_proto::registry::build;
    use vault_proto::registry::chain::verify_chain_with;
    use vault_proto::registry::device::{DeviceIdentity, SoftwareDevice, PLATFORM_IOS, PLATFORM_MACOS};

    const VID: [u8; 16] = [0x5A; 16];

    /// RG-18 (§4.4 S-4): a total-loss finalize appends the epoch, then one
    /// revoke per prior active device, ascending, each authorized by the
    /// epoch's device — exactly that. Missing, extra, out-of-order or
    /// otherwise-authorized revokes are refused; an unsigned one fails the
    /// chain itself.
    #[test]
    fn rg18_finalize_needs_every_revoke_in_order() {
        let (a, b) = (SoftwareDevice::generate("A", PLATFORM_MACOS), SoftwareDevice::generate("B", PLATFORM_IOS));
        let g = build::genesis(&a).unwrap();
        let st0 = verify_chain_with(&[g.clone()], &VID, &EpochPolicy::CheckpointAnchored).unwrap();
        let en = build::enroll(&st0, &a, &b).unwrap();
        let base = vec![g, en];
        let st = verify_chain_with(&base, &VID, &EpochPolicy::CheckpointAnchored).unwrap();
        let fresh = SoftwareDevice::generate("New", PLATFORM_MACOS);
        let epoch = build::recovery_epoch(&st, VID, [0x11; 32], &SecretBytes::new([0x22; 32]), &fresh).unwrap();
        let mut chain = base.clone();
        chain.push(epoch.clone());
        let st1 = verify_chain_with(&chain, &VID, &EpochPolicy::CheckpointAnchored).unwrap();
        let mut ids = vec![a.device_id(), b.device_id()];
        ids.sort();
        let r1 = build::revoke(&st1, &fresh, ids[0]).unwrap();
        let mut c2 = chain.clone();
        c2.push(r1.clone());
        let st2 = verify_chain_with(&c2, &VID, &EpochPolicy::CheckpointAnchored).unwrap();
        let r2 = build::revoke(&st2, &fresh, ids[1]).unwrap();
        let active = [a.device_id(), b.device_id()];
        assert!(finalize_shape(st.epoch, &active, &[epoch.clone(), r1.clone(), r2.clone()]), "the complete set");
        assert!(!finalize_shape(st.epoch, &active, &[epoch.clone(), r1.clone()]), "a revoke missing");
        assert!(!finalize_shape(st.epoch, &active, &[epoch.clone()]), "no revokes");
        assert!(!finalize_shape(st.epoch, &active[..1], &[epoch.clone(), r1.clone(), r2.clone()]), "an extra revoke");
        assert!(!finalize_shape(st.epoch, &active, &[epoch.clone(), r2.clone(), r1.clone()]), "out of order");
        assert!(!finalize_shape(st.epoch + 1, &active, &[epoch.clone(), r1.clone(), r2.clone()]), "the wrong epoch number");
        // A revoke authorized by an old device instead of the epoch's.
        let by_old = build::revoke(&st1, &a, ids[0]).unwrap();
        assert!(!finalize_shape(st.epoch, &active, &[epoch.clone(), by_old, r2.clone()]), "authorized by another device");
        // An unsigned revoke never gets past the chain.
        let mut unsigned = r1;
        unsigned.signature = None;
        let mut bad = chain;
        bad.push(unsigned);
        assert!(verify_chain_with(&bad, &VID, &EpochPolicy::CheckpointAnchored).is_err(), "unsigned");
    }
}
