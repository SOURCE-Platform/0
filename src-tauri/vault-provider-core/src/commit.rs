//! `POST /v2/vaults/{vid}/state` (spec v0.4 §11.3): authenticate, then
//! steps 2–10 — idempotency, precondition, validation, blob writes, one
//! CAS on the state object (retried from step 2 on a lost race). `create`
//! goes through the §11.3.1 claim protocol instead of a plain CAS.

use serde_json::json;
use sha2::{Digest, Sha256};
use vault_proto::backup::manifest::SignedManifest;
use vault_proto::crypto::registry::EntryKind;
use vault_proto::errors::ErrorCode;
use vault_proto::header::Hex32;
use vault_proto::registry::file;
use vault_proto::state::{StateTransition, TransitionKind};

use crate::auth::{GenesisSigner, Incoming, Signer};
use crate::model::{Recent, Reject, Response, VaultStateDoc};
use crate::Provider;

const CAS_ATTEMPTS: usize = 8;
const RECENT_KEEP: usize = 3;

impl Provider {
    pub(crate) fn state_commit(&self, inc: &Incoming<'_>) -> Result<Response, Reject> {
        let loaded = self.load_state(&inc.vault_id)?;
        let parsed = StateTransition::decode(inc.body);
        let genesis = match (&parsed, &loaded) {
            (Ok(t), _) if t.kind == TransitionKind::Create => Some(genesis_signer(t)?),
            (Err(_), None) => return Err(ErrorCode::AuthInvalid.into()),
            _ => None,
        };
        let state_for_auth = if genesis.is_some() { None } else { loaded.as_ref().map(|(s, _)| s) };
        let a = self.authenticate(inc, state_for_auth, genesis.as_ref())?;
        let t = parsed?;
        if a.req.expected_state != Some(t.expected_state) || t.vault_id != inc.vault_id {
            return Err(ErrorCode::AuthInvalid.into());
        }
        let class_ok = matches!(
            (t.kind, a.signer),
            (TransitionKind::Create | TransitionKind::Publish, Signer::Device(_)) | (TransitionKind::Finalize, Signer::Recovery(_))
        );
        if !class_ok {
            return Err(ErrorCode::DeviceNotAuthorized.into());
        }
        let body_sha: [u8; 32] = Sha256::digest(inc.body).into();
        if t.kind == TransitionKind::Create {
            let v = self.validate(None, &t, &a.signer)?;
            self.write_blobs(&inc.vault_id, &v.blobs)?;
            return self.create_with_claim(&t, v.state, body_sha, inc.now);
        }
        let mut current = loaded;
        for _ in 0..CAS_ATTEMPTS {
            let (cur, etag) = current.take().ok_or(ErrorCode::AuthInvalid)?;
            if let Some(r) = replayed(&cur, &t, &body_sha)? {
                return Ok(r);
            }
            if t.expected_state != cur.state_commit.0 {
                return Err(moved(&cur));
            }
            let v = self.validate(Some(&cur), &t, &a.signer)?;
            self.write_blobs(&inc.vault_id, &v.blobs)?;
            let next = successor(&cur, v.state, &t, body_sha)?;
            match self.state.replace(&inc.vault_id, &next.bytes(), &etag) {
                Ok(Some(_)) => return Ok(Response::json(200, json!(next.result()))),
                Ok(None) => current = self.load_state(&inc.vault_id)?,
                Err(_) => return Err(ErrorCode::BackupUnavailable.into()),
            }
        }
        Err(ErrorCode::BackupUnavailable.into())
    }

    pub(crate) fn load_state(&self, vid: &[u8; 16]) -> Result<Option<(VaultStateDoc, crate::stores::Etag)>, Reject> {
        match self.state.load(vid) {
            Ok(Some((b, e))) => Ok(Some((VaultStateDoc::parse(&b)?, e))),
            Ok(None) => Ok(None),
            Err(_) => Err(ErrorCode::BackupUnavailable.into()),
        }
    }

    pub(crate) fn write_blobs(&self, vid: &[u8; 16], blobs: &[([u8; 32], Vec<u8>)]) -> Result<(), Reject> {
        for (sha, bytes) in blobs {
            self.blobs.put_if_absent(vid, sha, bytes).map_err(|_| ErrorCode::BackupUnavailable)?;
        }
        Ok(())
    }
}

/// Step 2 (and §11.8 finalize replay): a byte-identical retry returns the
/// stored result; a different finalize for a passed generation conflicts.
fn replayed(cur: &VaultStateDoc, t: &StateTransition, body_sha: &[u8; 32]) -> Result<Option<Response>, Reject> {
    if let Some(r) = cur.recent.iter().find(|r| r.body_sha256.0 == *body_sha) {
        return Ok(Some(Response::json(200, json!(r.result))));
    }
    if t.kind == TransitionKind::Finalize {
        let m = SignedManifest::decode(&t.manifest).map_err(|_| ErrorCode::ManifestInvalid)?;
        let key = m.generation.saturating_sub(1).to_string();
        if let Some(done) = cur.finalized.get(&key) {
            if done.body_sha256.0 == *body_sha {
                return Ok(Some(Response::json(200, json!(done.result))));
            }
            return Err(ErrorCode::FinalizeConflict.into());
        }
    }
    Ok(None)
}

pub(crate) fn moved(cur: &VaultStateDoc) -> Reject {
    Reject(ErrorCode::StateMoved, json!({ "state_commit": cur.state_commit, "generation": cur.generation }))
}

/// Step 9: the new state with its bookkeeping and commitment.
fn successor(cur: &VaultStateDoc, mut next: VaultStateDoc, t: &StateTransition, body_sha: [u8; 32]) -> Result<VaultStateDoc, Reject> {
    next.retained = std::iter::once(cur.current_ref()).chain(cur.retained.first().cloned()).collect();
    next.finalized = cur.finalized.clone();
    next.state_commit = Hex32(next.compute_commit()?);
    let entry = Recent { body_sha256: Hex32(body_sha), result: next.result() };
    if t.kind == TransitionKind::Finalize {
        next.finalized.insert(cur.generation.to_string(), entry.clone());
    }
    next.recent = cur.recent.iter().cloned().chain(std::iter::once(entry)).collect();
    let skip = next.recent.len().saturating_sub(RECENT_KEEP);
    next.recent.drain(..skip);
    Ok(next)
}

/// The genesis device of a `create`'s inline registry (§11.3 step 1).
fn genesis_signer(t: &StateTransition) -> Result<GenesisSigner, Reject> {
    let bad = || Reject::from(ErrorCode::AuthInvalid);
    for b in &t.bootstrap_blobs {
        if let Ok(entries) = file::decode(b) {
            let g = entries.first().filter(|g| g.kind == EntryKind::Genesis).ok_or_else(bad)?;
            return Ok(GenesisSigner { device_id: g.device_id, sign_pub: g.sign_pub.ok_or_else(bad)? });
        }
    }
    Err(bad())
}
