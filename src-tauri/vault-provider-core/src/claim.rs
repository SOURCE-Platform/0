//! §11.3.1 `create`: bind a recovery handle to a new vault without a
//! cross-key transaction (C1–C4). A crash after C2 never burns the handle;
//! a crash after C3 is completed by our retry and protected by the third
//! liveness clause; the loser of a reclaim race rolls back its unbound
//! generation-1 state (a provider-internal compensation, not a client
//! delete capability).

use serde_json::json;
use vault_proto::crypto::hex;
use vault_proto::errors::ErrorCode;
use vault_proto::header::{Hex16, Hex32};
use vault_proto::state::StateTransition;

use crate::model::{Claim, Recent, Reject, Response, VaultStateDoc};
use crate::stores::Etag;
use crate::Provider;

/// Unbound claims stay live for G = 24 h.
pub const CLAIM_GRACE: u64 = 24 * 3600;
const ATTEMPTS: usize = 8;

pub fn claim_key(handle_key: &[u8; 32]) -> String {
    format!("v2/handles/{}", hex::encode(handle_key))
}

impl Provider {
    pub(crate) fn create_with_claim(
        &self,
        t: &StateTransition,
        mut state: VaultStateDoc,
        body_sha: [u8; 32],
        now: u64,
    ) -> Result<Response, Reject> {
        let unavailable = |_| Reject::from(ErrorCode::BackupUnavailable);
        let hk = t.handle_key.ok_or(ErrorCode::ManifestInvalid)?;
        let key = claim_key(&hk);
        let vid = Hex16(t.vault_id);
        for _ in 0..ATTEMPTS {
            // C1
            let (claim_id, claim_etag) = match self.ops.get(&key).map_err(unavailable)? {
                Some((b, etag)) => {
                    let c: Claim = serde_json::from_slice(&b).map_err(|_| ErrorCode::Internal)?;
                    if c.vault_id == vid {
                        (c.claim_id, etag)
                    } else if self.claim_live(&c, now)? {
                        return Err(ErrorCode::HandleTaken.into());
                    } else {
                        // C2′ reclaim
                        let fresh = new_claim(vid, now);
                        match self.ops.replace(&key, &claim_bytes(&fresh), &etag).map_err(unavailable)? {
                            Some(e) => (fresh.claim_id, e),
                            None => continue,
                        }
                    }
                }
                None => {
                    // C2
                    let fresh = new_claim(vid, now);
                    match self.ops.create(&key, &claim_bytes(&fresh)).map_err(unavailable)? {
                        Some(e) => (fresh.claim_id, e),
                        None => continue,
                    }
                }
            };
            // C3
            state.claim_id = claim_id;
            state.handle_key = Hex32(hk);
            state.state_commit = Hex32(state.compute_commit()?);
            state.recent = vec![Recent { body_sha256: Hex32(body_sha), result: state.result() }];
            let state_etag = match self.state.create(&t.vault_id, &state.bytes()).map_err(unavailable)? {
                Some(e) => Some(e),
                None => {
                    let (existing, _) = self.load_state(&t.vault_id)?.ok_or(ErrorCode::BackupUnavailable)?;
                    let ours = existing.claim_id == claim_id && existing.recent.iter().any(|r| r.body_sha256.0 == body_sha);
                    if !ours {
                        return Err(crate::commit::moved(&existing));
                    }
                    state = existing;
                    None
                }
            };
            // C4
            let bound = Claim { vault_id: vid, claim_id, created_at: now, status: "bound".into() };
            if self.ops.replace(&key, &claim_bytes(&bound), &claim_etag).map_err(unavailable)?.is_some() {
                return Ok(Response::json(200, json!(state.result())));
            }
            if let Some((b, _)) = self.ops.get(&key).map_err(unavailable)? {
                let c: Claim = serde_json::from_slice(&b).map_err(|_| ErrorCode::Internal)?;
                if c.vault_id == vid && c.claim_id == claim_id && c.status == "bound" {
                    return Ok(Response::json(200, json!(state.result())));
                }
                if c.vault_id != vid {
                    self.rollback(&t.vault_id, state_etag.as_ref())?;
                    return Err(ErrorCode::HandleTaken.into());
                }
            }
        }
        Err(ErrorCode::BackupUnavailable.into())
    }

    /// Live iff bound; or pending and younger than G; or pending and its
    /// vault's state exists with the same `claim_id`.
    pub(crate) fn claim_live(&self, c: &Claim, now: u64) -> Result<bool, Reject> {
        if c.status == "bound" || now.saturating_sub(c.created_at) < CLAIM_GRACE {
            return Ok(true);
        }
        Ok(self.load_state(&c.vault_id.0)?.is_some_and(|(s, _)| s.claim_id == c.claim_id))
    }

    fn rollback(&self, vid: &[u8; 16], etag: Option<&Etag>) -> Result<(), Reject> {
        let etag = match etag {
            Some(e) => e.clone(),
            None => match self.load_state(vid)? {
                Some((s, e)) if s.generation == 1 => e,
                _ => return Ok(()),
            },
        };
        let _ = self.state.delete(vid, &etag);
        Ok(())
    }
}

fn new_claim(vault_id: Hex16, now: u64) -> Claim {
    let mut id = [0u8; 16];
    getrandom::fill(&mut id).expect("OS RNG");
    Claim { vault_id, claim_id: Hex16(id), created_at: now, status: "pending".into() }
}

fn claim_bytes(c: &Claim) -> Vec<u8> {
    serde_json::to_vec(c).expect("claim serializes")
}
