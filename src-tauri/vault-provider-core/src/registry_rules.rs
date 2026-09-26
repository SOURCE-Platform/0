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
        (TransitionKind::Finalize, Some((cur, _))) => finalize_shape(cur, appended),
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
fn finalize_shape(cur: &VaultStateDoc, appended: &[RegistryEntry]) -> bool {
    let Some((epoch, revokes)) = appended.split_first() else {
        return false;
    };
    if epoch.kind != EntryKind::RecoveryEpoch || epoch.epoch != cur.epoch + 1 {
        return false;
    }
    let mut prior: Vec<[u8; 16]> = cur.active_devices.iter().map(|d| d.device_id.0).collect();
    prior.sort();
    revokes.len() == prior.len()
        && revokes.iter().zip(&prior).all(|(r, id)| {
            r.kind == EntryKind::Revoke && &r.device_id == id && r.authorizer == Some(epoch.device_id)
        })
}
