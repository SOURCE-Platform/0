//! Device revocation as one local journaled commit (spec v0.4 §11.4,
//! §12 scenario 8): the registry `revoke` entry, a VK rotation that
//! re-envelopes every surviving device and drops the target's envelope,
//! both recovery classes re-keyed (a fresh `kdf.salt`/`auth_salt_mp` with
//! `password.wrap` re-sealed under the same MP, and a new RK with a new
//! `auth_salt_rk`), `Admit(target)` fixed (§3.2), and the
//! security-driven `pending_remote` record. One `publish` then cuts the
//! device off at the provider. The IPC op (`devices.rs`) adds presence,
//! the MP panel and the acknowledged Recovery Key sheet around this.

use super::recovery_ops::prove_mp;
use crate::crypto::secret::SecretBytes;
use crate::device::rotate::EnvelopePlan;
use crate::errors::ErrorCode;
use crate::registry::chain::{self, EpochPolicy};
use crate::registry::device::DeviceIdentity;
use crate::registry::{build, file as registry_file, log};
use crate::storage::header::Hex32;
use crate::storage::revisions::uuid_string;
use crate::storage::rotation::{self, MpWrap, RkWrap};
use crate::storage::VaultStore;
use crate::sync::change::RemoteChange;
use crate::sync::pending::{self, Base, PendingOp};

pub struct Revoked {
    pub store: VaultStore,
    pub vk: SecretBytes<32>,
    pub registry_head: [u8; 32],
    pub vk_generation: u32,
}

/// `mp` is verified against the committed `password.wrap` (a typo can
/// never become the vault's MP); `new_rk` is the acknowledged new RK.
pub fn revoke(
    mut store: VaultStore,
    vk: &SecretBytes<32>,
    me: &dyn DeviceIdentity,
    target: [u8; 16],
    mp: &[u8],
    new_rk: &SecretBytes<32>,
) -> Result<Revoked, ErrorCode> {
    let dir = store.dir.clone();
    let vid = store.header.vault_id.0;
    if target == me.device_id() {
        return Err(ErrorCode::InvalidInput); // no vault without an authorizer
    }
    let state = log::read_state(&dir, &vid, &EpochPolicy::CheckpointAnchored)?;
    if state.active_device(&target).is_none() {
        return Err(ErrorCode::NotFound);
    }
    prove_mp(&store, mp)?;
    let base = pending::load(&store.conn)?.map_or_else(|| Base::of(&store.header), |p| p.base);
    let mut entries = state.entries.clone();
    entries.push(build::revoke(&state, me, target)?);
    let after = chain::verify_chain_with(&entries, &vid, &EpochPolicy::CheckpointAnchored)?;
    let envelopes = EnvelopePlan {
        vault_id: vid,
        devices: after.devices.iter().filter(|d| !d.revoked).map(|d| (d.device_id, d.agree_pub)).collect(),
        fresh: Vec::new(),
    };
    let change = RemoteChange {
        envelopes: Some(&envelopes),
        op: PendingOp::Revocation,
        security_driven: true,
        base,
        mp: Some(mp),
        rk: Some(new_rk),
        revoke: Some((target, uuid_string(&me.device_id()))),
        registry: Some(registry_file::encode(&entries)?),
    };
    // The new head rides in the staged header and manifest.
    store.header.registry_head = Hex32(after.head);
    store.manifest.registry_head = Hex32(after.head);
    let rotated = rotation::rotate(store, vk, MpWrap::Fresh(mp), RkWrap::SealNew(new_rk), Some(&change), None)?;
    // The target's envelope is not in the staged set; remove the old file.
    crate::device::envelope::remove_envelope(&dir, &target);
    let store = VaultStore::open(&dir)?;
    Ok(Revoked { store, vk: rotated.new_vk, registry_head: after.head, vk_generation: rotated.vk_generation })
}
