//! Staging a state transition from the local vault (spec v0.4 §11.3):
//! `create` (setup, all blobs inline) and `publish` (blobs uploaded
//! first). The helper computes the `state_commit` it expects the provider
//! to return and accepts a commit result only if it matches (§11.2: the
//! helper recomputes every commitment itself).

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use vault_proto::state::{recovery_auth_digest, state_commit, RecoveryAuthEntry, TransitionKind};

use super::local::stage_local;
use super::seen::{self, merge_auth, Seen};
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::registry::chain::RegistryState;
use crate::registry::device::DeviceIdentity;
use crate::storage::VaultStore;

/// A fully staged transition: everything main needs to upload and post.
pub struct Staging {
    pub kind: TransitionKind,
    /// Every blob the new state references (for `create` they also travel
    /// inline in the body).
    pub blobs: BTreeMap<[u8; 32], Vec<u8>>,
    pub body: Vec<u8>,
    pub body_sha256: [u8; 32],
    pub expected_state: [u8; 32],
    pub generation: u64,
    pub manifest_hash: [u8; 32],
    pub new_state_commit: [u8; 32],
    pub new_auth: Vec<RecoveryAuthEntry>,
    /// The transition carries the pending local change (§11.3.2): its
    /// commit is REMOTE_COMMITTED for it.
    pub carries_pending: bool,
}

/// Finish a staged state into a transition (also used by §11.8 finalize).
pub fn finish_transition(
    kind: TransitionKind,
    staged: vault_proto::backup::stage::Staged,
    expected_state: [u8; 32],
    base_auth: &[RecoveryAuthEntry],
    updates: Vec<RecoveryAuthEntry>,
    handle_key: Option<[u8; 32]>,
) -> Result<Staging, ErrorCode> {
    let new_auth = merge_auth(base_auth, &updates);
    let manifest_hash = staged.manifest.hash();
    let new_state_commit = state_commit(
        &staged.manifest.vault_id,
        staged.manifest.generation,
        &manifest_hash,
        &staged.checkpoint_hash(),
        &recovery_auth_digest(&new_auth)?,
    );
    let body = staged.transition(kind, expected_state, updates, handle_key).encode()?;
    Ok(Staging {
        kind,
        body_sha256: Sha256::digest(&body).into(),
        body,
        expected_state,
        generation: staged.manifest.generation,
        manifest_hash,
        new_state_commit,
        new_auth,
        blobs: staged.blobs,
        carries_pending: false,
    })
}

/// The genesis `create` (§11.3 bootstrap): generation 1, no records
/// beyond what the vault holds, both recovery classes registered.
pub fn stage_create(
    store: &VaultStore,
    registry: &RegistryState,
    vk: &SecretBytes<32>,
    signer: &dyn DeviceIdentity,
    handle_key: [u8; 32],
    updates: Vec<RecoveryAuthEntry>,
) -> Result<Staging, ErrorCode> {
    let staged = stage_local(store, registry, 1, [0u8; 32], signer, vk, &[])?;
    finish_transition(TransitionKind::Create, staged, [0u8; 32], &[], updates, Some(handle_key))
}

/// A `publish` on top of the last accepted state. A pending local change
/// whose base is still current rides along with its public recovery-auth
/// updates (§11.3.2); one that needs the user does not (its wraps were
/// replaced by the adopted state).
pub fn stage_publish(
    store: &VaultStore,
    registry: &RegistryState,
    vk: &SecretBytes<32>,
    signer: &dyn DeviceIdentity,
    seen: &Seen,
    mut updates: Vec<RecoveryAuthEntry>,
) -> Result<Staging, ErrorCode> {
    let pending = super::pending::load(&store.conn)?.filter(|p| !p.needs_user);
    if let Some(p) = &pending {
        let carried = Seen { recovery_auth: p.recovery_auth_updates.clone(), ..seen.clone() }.auth_entries()?;
        updates = merge_auth(&carried, &updates);
    }
    let staged = stage_local(store, registry, seen.generation + 1, seen.manifest_hash.0, signer, vk, &[])?;
    let mut st = finish_transition(TransitionKind::Publish, staged, seen.state_commit.0, &seen.auth_entries()?, updates, None)?;
    st.carries_pending = pending.is_some();
    Ok(st)
}

/// A `200` for this staging: accept it only if the provider's result is
/// the commitment the helper computed, then move the rollback floor.
pub fn committed(store: &VaultStore, st: &Staging, generation: u64, commit: [u8; 32]) -> Result<Seen, ErrorCode> {
    if generation != st.generation || commit != st.new_state_commit {
        return Err(ErrorCode::ManifestMismatch);
    }
    let s = Seen::with_auth(generation, st.manifest_hash, commit, &st.new_auth);
    seen::save(&store.conn, &s)?;
    if st.carries_pending {
        super::pending::clear(&store.conn)?; // REMOTE_COMMITTED
    }
    Ok(s)
}

/// The public recovery-auth updates for the classes whose secret the
/// caller holds right now (§11.4 D-11): derived transiently from PK /
/// RK_bytes and the header's class salts; only public keys leave here.
pub fn recovery_updates(
    header: &crate::storage::header::Header,
    pk: Option<&SecretBytes<32>>,
    rk: Option<&SecretBytes<32>>,
) -> Result<Vec<RecoveryAuthEntry>, ErrorCode> {
    use crate::crypto::recovery_auth::{derive, RecoveryClass};
    let vid = header.vault_id.0;
    let mut out = Vec::new();
    for (class, secret, salt) in [(RecoveryClass::Mp, pk, header.auth_salt_mp.0), (RecoveryClass::Rk, rk, header.auth_salt_rk.0)] {
        if let Some(s) = secret {
            let k = derive(class, s, &salt, &vid).map_err(|_| ErrorCode::Internal)?;
            out.push(RecoveryAuthEntry { class, public: k.public, salt });
        }
    }
    Ok(out)
}
