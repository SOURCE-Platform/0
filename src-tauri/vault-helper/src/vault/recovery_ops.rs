//! Trusted-device recovery flows (spec §12 scenarios 5–7), run by the
//! helper on an UNLOCKED vault. The IPC ops in `vault::rk_ops` call these
//! after the panel collected the secrets; the FsBackupStore rehearsals
//! call them directly.
//!
//! - Scenario 5 (MP forgotten): re-wrap the resident VK under a new MP —
//!   no rotation; `password.wrap` is replaced atomically (no window in
//!   which both the old and the new MP work).
//! - Scenarios 6/7 (RK lost / suspected stolen): new RK + full VK
//!   rotation (v0.3 C12: re-wrapping alone is insufficient). Re-sealing
//!   `password.wrap` under the new VK needs PK, so the current MP is
//!   collected too (the spec leaves that prompt implicit — documented).

use crate::crypto::kdf::{self, Argon2Params};
use crate::crypto::secret::{random_secret, SecretBytes};
use crate::crypto::wrap::{self, RecoveryWrapPayload};
use crate::errors::ErrorCode;
use crate::storage::header::KdfBlock;
use crate::storage::rotation::{self, MpWrap, RkWrap, RotationOutcome};
use crate::storage::store::{now_epoch, PASSWORD_WRAP_NAME};
use crate::storage::VaultStore;

/// PK for the current MP, proven against the live wrap.
pub fn prove_mp(store: &VaultStore, mp: &[u8]) -> Result<SecretBytes<32>, ErrorCode> {
    let bytes = std::fs::read(store.dir.join(PASSWORD_WRAP_NAME)).map_err(|_| ErrorCode::WrapCorrupt)?;
    let file: wrap::PasswordWrapFile = serde_json::from_slice(&bytes).map_err(|_| ErrorCode::WrapCorrupt)?;
    let (salt, params) = crate::vault::setup::wrap_kdf(&file)?;
    let pk = kdf::derive_pk(mp, &salt, params).map_err(|_| ErrorCode::Internal)?;
    wrap::open_wrap_mp(&file, &pk, &store.header.vault_id.0).map_err(|_| ErrorCode::WrongCredential)?;
    Ok(pk)
}

pub struct RkRotation {
    pub new_rk: SecretBytes<32>,
    pub rotation: RotationOutcome,
}

/// Scenarios 6/7: generate RK′ and rotate the VK — every active device
/// re-enveloped (§2.10), the RK class re-keyed, and the RK-replacement
/// `pending_remote` component recorded in the same commit (§11.3.2;
/// `security_driven` for a suspected-stolen RK). Consumes the store (the
/// caller reopens it under the new VK).
pub fn rotate_recovery_key(
    store: VaultStore,
    vk: &SecretBytes<32>,
    pk: &SecretBytes<32>,
    security_driven: bool,
) -> Result<RkRotation, ErrorCode> {
    let new_rk = random_secret();
    let rotation = rotate_with_rk(store, vk, pk, &new_rk, security_driven)?;
    Ok(RkRotation { new_rk, rotation })
}

/// The rotation behind an RK replacement, for a caller that already
/// issued (and had the user acknowledge) `new_rk`.
pub fn rotate_with_rk(
    store: VaultStore,
    vk: &SecretBytes<32>,
    pk: &SecretBytes<32>,
    new_rk: &SecretBytes<32>,
    security_driven: bool,
) -> Result<RotationOutcome, ErrorCode> {
    use crate::registry::chain::EpochPolicy;
    use crate::sync::pending::{self, Base, PendingOp};
    let vid = store.header.vault_id.0;
    let reg = crate::registry::log::read_state(&store.dir, &vid, &EpochPolicy::CheckpointAnchored)?;
    let envelopes = crate::device::rotate::EnvelopePlan {
        vault_id: vid,
        devices: reg.devices.iter().filter(|d| !d.revoked).map(|d| (d.device_id, d.agree_pub)).collect(),
        fresh: Vec::new(),
    };
    let change = crate::sync::change::RemoteChange {
        envelopes: Some(&envelopes),
        op: PendingOp::RkReplacement,
        security_driven,
        base: pending::load(&store.conn)?.map_or_else(|| Base::of(&store.header), |p| p.base),
        mp: None,
        rk: Some(new_rk),
        revoke: None,
        registry: None,
    };
    rotation::rotate(store, vk, MpWrap::Reseal(pk), RkWrap::SealNew(new_rk), Some(&change), None)
}

/// An MP change in either mode (§1.5; §12 scenario 5 `reset` passes no
/// old MP): a fresh kdf salt and MP-class auth salt, `password.wrap`
/// re-sealed over the resident VK, and the MP-change `pending_remote`
/// component with its public recovery-auth update — one journaled commit
/// (§11.3.2). No VK rotation. Returns the reopened store.
pub fn change_mp(store: VaultStore, vk: &SecretBytes<32>, old_mp: Option<&[u8]>, new_mp: &[u8]) -> Result<VaultStore, ErrorCode> {
    use crate::sync::change::{seen_auth, updates_for};
    use crate::sync::pending::{self, Base, PendingOp};
    if let Some(old) = old_mp {
        prove_mp(&store, old)?;
    }
    let dir = store.dir.clone();
    let salt = crate::crypto::secret::random_salt();
    let pk = kdf::derive_pk(new_mp, &salt, Argon2Params::V1).map_err(|_| ErrorCode::Internal)?;
    let payload = RecoveryWrapPayload { vk: SecretBytes::new(*vk.expose()), wrapped_at: now_epoch(), vk_generation: store.header.vk_generation };
    let file = wrap::seal_wrap_mp(&payload, &pk, &store.header.vault_id.0, Argon2Params::V1, &salt).map_err(|_| ErrorCode::Internal)?;
    let wrap_json = serde_json::to_vec_pretty(&file).map_err(|_| ErrorCode::Internal)?;
    let mut next = store.header.clone();
    next.kdf = KdfBlock::frozen(salt);
    next.auth_salt_mp = crate::storage::header::Hex16::random();
    let base = pending::load(&store.conn)?.map_or_else(|| Base::of(&store.header), |p| p.base);
    let updates = seen_auth(&updates_for(&next, Some(new_mp), None)?);
    let record = move |c: &rusqlite::Connection| pending::add(c, PendingOp::MpChange, false, base.clone(), updates.clone(), now_epoch()).map(|_| ());
    crate::storage::adopt::commit_singleton_change(store, &next, &wrap_json, &record)?;
    VaultStore::open(&dir)
}
