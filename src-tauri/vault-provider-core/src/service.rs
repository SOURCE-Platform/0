//! Route dispatch (spec v0.4 §11.3 routes). Transport-neutral: the
//! in-process transport and the axum adapter both call `Provider::handle`.

use serde_json::json;
use sha2::{Digest, Sha256};
use vault_proto::b64;
use vault_proto::crypto::hex;
use vault_proto::errors::ErrorCode;
use vault_proto::request::Operation;

use crate::auth::Incoming;
use crate::model::{Reject, Response};
use crate::validate::CAP_INDEX;
use crate::Provider;

pub struct Request<'a> {
    pub method: &'a str,
    /// Path only — a query string is never part of a signed request.
    pub path: &'a str,
    /// The `Ov0-Auth` header value, if any.
    pub auth: Option<&'a str>,
    pub body: &'a [u8],
    pub now: u64,
    pub client_ip: &'a str,
}

enum Route {
    Vault([u8; 16], Operation, Option<[u8; 32]>),
    Locate,
}

fn route(method: &str, path: &str) -> Option<Route> {
    if method == "POST" && path == "/v2/recover/locate" {
        return Some(Route::Locate);
    }
    let rest = path.strip_prefix("/v2/vaults/")?;
    let (vid, tail) = rest.split_once('/')?;
    let vid = vault_proto::header::strict_hex::<16>(vid)?;
    let (op, blob) = match (method, tail) {
        ("GET", "state") => (Operation::StateGet, None),
        ("POST", "state") => (Operation::StateCommit, None),
        (m, t) => {
            let sha = vault_proto::header::strict_hex::<32>(t.strip_prefix("blobs/")?)?;
            match m {
                "GET" => (Operation::BlobGet, Some(sha)),
                "PUT" => (Operation::BlobPut, Some(sha)),
                _ => return None,
            }
        }
    };
    // Only canonical paths route (lowercase hex, no extra segments).
    (op.route(&vid, blob.as_ref())?.1 == path).then_some(Route::Vault(vid, op, blob))
}

impl Provider {
    pub fn handle(&self, r: &Request<'_>) -> Response {
        let res = match route(r.method, r.path) {
            None => Err(Reject::from(ErrorCode::NotFound)),
            Some(Route::Locate) => self.locate(r.body, r.client_ip, r.now),
            Some(Route::Vault(vid, op, blob)) => {
                let inc = Incoming { method: r.method, path: r.path, vault_id: vid, operation: op, auth: r.auth, body: r.body, now: r.now };
                match op {
                    Operation::StateCommit => self.state_commit(&inc),
                    Operation::StateGet => self.state_get(&inc),
                    Operation::BlobGet => self.blob_get(&inc, &blob.expect("routed")),
                    Operation::BlobPut => self.blob_put(&inc, &blob.expect("routed")),
                }
            }
        };
        res.unwrap_or_else(Response::from)
    }

    fn state_get(&self, inc: &Incoming<'_>) -> Result<Response, Reject> {
        let loaded = self.load_state(&inc.vault_id)?;
        self.authenticate(inc, loaded.as_ref().map(|(s, _)| s), None)?;
        let (s, _) = loaded.ok_or(ErrorCode::AuthInvalid)?;
        let get = |sha: &[u8; 32]| -> Result<Vec<u8>, Reject> {
            Ok(self.blobs.get(&inc.vault_id, sha).map_err(|_| ErrorCode::BackupUnavailable)?.ok_or(ErrorCode::BackupUnavailable)?)
        };
        let recovery_auth: Vec<_> = s
            .recovery_auth
            .entries()
            .iter()
            .map(|e| json!({ "class": e.class.code(), "pub": hex::encode(e.public), "salt": hex::encode(e.salt) }))
            .collect();
        Ok(Response::json(
            200,
            json!({
                "generation": s.generation,
                "state_commit": s.state_commit,
                "manifest": b64::encode(&get(&s.manifest_hash.0)?),
                "checkpoint": b64::encode(&get(&s.checkpoint_hash.0)?),
                "vk_generation": s.vk_generation,
                "recovery_auth": recovery_auth,
            }),
        ))
    }

    fn blob_get(&self, inc: &Incoming<'_>, sha: &[u8; 32]) -> Result<Response, Reject> {
        let loaded = self.load_state(&inc.vault_id)?;
        self.authenticate(inc, loaded.as_ref().map(|(s, _)| s), None)?;
        let b = self.blobs.get(&inc.vault_id, sha).map_err(|_| ErrorCode::BackupUnavailable)?;
        Ok(Response::bytes(b.ok_or(ErrorCode::NotFound)?))
    }

    /// Create-only; the hash is recomputed; never for a vault without
    /// state (the signer cannot resolve, so it is the generic `401`).
    fn blob_put(&self, inc: &Incoming<'_>, sha: &[u8; 32]) -> Result<Response, Reject> {
        let loaded = self.load_state(&inc.vault_id)?;
        self.authenticate(inc, loaded.as_ref().map(|(s, _)| s), None)?;
        if inc.body.len() as u64 > CAP_INDEX {
            return Err(ErrorCode::TooLarge.into());
        }
        if <[u8; 32]>::from(Sha256::digest(inc.body)) != *sha {
            return Err(ErrorCode::HashMismatch.into());
        }
        self.blobs.put_if_absent(&inc.vault_id, sha, inc.body).map_err(|_| ErrorCode::BackupUnavailable)?;
        Ok(Response::json(200, json!({})))
    }
}
