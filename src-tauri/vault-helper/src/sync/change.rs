//! A wrap-bearing local change and its remote-completion record in one
//! journaled commit (spec v0.4 §11.3.2, §2.10). Used as the rotation's
//! extra staging: it re-seals the surviving devices' envelopes, fixes
//! `Admit(D)` for a revocation (§3.2), and adds the `pending_remote`
//! component with the public recovery-auth updates derived from the new
//! header's salts — all inside the staged DB, so a crash leaves either the
//! old vault or the new one with its pending record, never a mix.

use std::path::Path;

use rusqlite::Connection;
use vault_proto::state::RecoveryAuthEntry;

use super::pending::{self, Base, PendingOp};
use super::seen::SeenAuth;
use crate::crypto::kdf;
use crate::crypto::recovery_auth::{derive, RecoveryClass};
use crate::crypto::secret::SecretBytes;
use crate::device::rotate::EnvelopePlan;
use crate::errors::ErrorCode;
use crate::storage::header::{kdf_params, Header, Hex16};
use crate::storage::revoked;
use crate::storage::rotation::ExtraStaging;
use crate::storage::store::now_epoch;

pub struct RemoteChange<'a> {
    pub envelopes: Option<&'a EnvelopePlan>,
    pub op: PendingOp,
    pub security_driven: bool,
    /// The remote singletons the change is built on (pre-change header).
    pub base: Base,
    /// The MP whose class is (re-)keyed under the new header's salts.
    pub mp: Option<&'a [u8]>,
    /// The new RK whose class is keyed under the new `auth_salt_rk`.
    pub rk: Option<&'a SecretBytes<32>>,
    /// `(revoked device, this device's author id)` for a revocation.
    pub revoke: Option<([u8; 16], String)>,
    /// A new registry file committed with the change (revocation).
    pub registry: Option<Vec<u8>>,
}

/// The public updates for `mp`/`rk` under `h`'s salts (§11.4 D-11).
pub fn updates_for(h: &Header, mp: Option<&[u8]>, rk: Option<&SecretBytes<32>>) -> Result<Vec<RecoveryAuthEntry>, ErrorCode> {
    let vid = h.vault_id.0;
    let mut out = Vec::new();
    if let Some(mp) = mp {
        let pk = kdf::derive_pk(mp, &h.kdf.salt.0, kdf_params(&h.kdf)).map_err(|_| ErrorCode::Internal)?;
        let k = derive(RecoveryClass::Mp, &pk, &h.auth_salt_mp.0, &vid).map_err(|_| ErrorCode::Internal)?;
        out.push(RecoveryAuthEntry { class: RecoveryClass::Mp, public: k.public, salt: h.auth_salt_mp.0 });
    }
    if let Some(rk) = rk {
        let k = derive(RecoveryClass::Rk, rk, &h.auth_salt_rk.0, &vid).map_err(|_| ErrorCode::Internal)?;
        out.push(RecoveryAuthEntry { class: RecoveryClass::Rk, public: k.public, salt: h.auth_salt_rk.0 });
    }
    Ok(out)
}

pub fn seen_auth(u: &[RecoveryAuthEntry]) -> Vec<SeenAuth> {
    u.iter().map(|e| SeenAuth { class: e.class.code(), public: crate::crypto::hex::encode(e.public), salt: Hex16(e.salt) }).collect()
}

impl ExtraStaging for RemoteChange<'_> {
    fn stage(&self, dir: &Path, new_vk: &SecretBytes<32>, new_vk_generation: u32) -> Result<Vec<String>, ErrorCode> {
        let mut names = match self.envelopes {
            Some(e) => e.stage(dir, new_vk, new_vk_generation)?,
            None => Vec::new(),
        };
        if let Some(reg) = &self.registry {
            let next = crate::storage::rotation_journal::next_path(dir, crate::VAULT_REGISTRY_NAME);
            crate::storage::store::write_atomic(&next, reg)?;
            names.push(crate::VAULT_REGISTRY_NAME.to_string());
        }
        Ok(names)
    }

    fn stage_db(&self, conn: &Connection, new_header: &Header) -> Result<(), ErrorCode> {
        if let Some((target, me)) = &self.revoke {
            revoked::record_local(conn, target, me)?;
        }
        let updates = updates_for(new_header, self.mp, self.rk)?;
        pending::add(conn, self.op, self.security_driven, self.base.clone(), seen_auth(&updates), now_epoch())?;
        Ok(())
    }
}
