//! `POST /v2/recover/locate` (spec v0.4 §11.5), unauthenticated. A handle
//! that resolves (bound claim whose vault state names it) returns the
//! committed header's public metadata; anything else returns the same shape
//! with deterministic fake values keyed by the provider-held pepper, which
//! carries no vault authority. Per-IP and per-handle limits are in memory.

use std::collections::HashMap;
use std::sync::Mutex;

use hmac::{Hmac, KeyInit, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use vault_proto::crypto::hex;
use vault_proto::errors::ErrorCode;
use vault_proto::header::{Hex16, KdfBlock};

use crate::claim::claim_key;
use crate::model::{Claim, Reject, Response};
use crate::Provider;

const LIMIT_WINDOW: u64 = 60;
const PER_IP: u32 = 30;
const PER_HANDLE: u32 = 10;

#[derive(Default)]
pub struct LocateLimits {
    inner: Mutex<HashMap<String, (u64, u32)>>,
}

impl LocateLimits {
    fn allow(&self, key: String, now: u64, limit: u32) -> bool {
        let mut m = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let window = now / LIMIT_WINDOW;
        let e = m.entry(key).or_insert((window, 0));
        if e.0 != window {
            *e = (window, 0);
        }
        e.1 += 1;
        e.1 <= limit
    }
}

impl Provider {
    pub(crate) fn locate(&self, body: &[u8], client_ip: &str, now: u64) -> Result<Response, Reject> {
        let v: Value = serde_json::from_slice(body).map_err(|_| ErrorCode::InvalidInput)?;
        let obj = v.as_object().filter(|o| o.len() == 1).ok_or(ErrorCode::InvalidInput)?;
        let hk: [u8; 32] = obj
            .get("handle_key")
            .and_then(Value::as_str)
            .and_then(vault_proto::header::strict_hex)
            .ok_or(ErrorCode::InvalidInput)?;
        if !self.locate_limits.allow(format!("ip:{client_ip}"), now, PER_IP)
            || !self.locate_limits.allow(format!("hk:{}", hex::encode(hk)), now, PER_HANDLE)
        {
            return Err(ErrorCode::RecoveryThrottled.into());
        }
        let real = self.resolve(&hk)?;
        let (vault_id, kdf, mp, rk) = match real {
            Some(x) => x,
            None => {
                // Equalize the store reads of the real path (best effort).
                let _ = self.load_state(&self.fake(b"vault_id", &hk));
                let kdf = KdfBlock::frozen(self.fake(b"kdf_salt", &hk));
                (Hex16(self.fake(b"vault_id", &hk)), kdf, Hex16(self.fake(b"auth_salt_mp", &hk)), Hex16(self.fake(b"auth_salt_rk", &hk)))
            }
        };
        Ok(Response::json(200, json!({ "vault_id": vault_id, "kdf": kdf, "auth_salt_mp": mp, "auth_salt_rk": rk })))
    }

    fn resolve(&self, hk: &[u8; 32]) -> Result<Option<(Hex16, KdfBlock, Hex16, Hex16)>, Reject> {
        let Some((b, _)) = self.ops.get(&claim_key(hk)).map_err(|_| ErrorCode::BackupUnavailable)? else {
            return Ok(None);
        };
        let Ok(c) = serde_json::from_slice::<Claim>(&b) else { return Ok(None) };
        if c.status != "bound" {
            return Ok(None);
        }
        Ok(self.load_state(&c.vault_id.0)?.and_then(|(s, _)| {
            (s.claim_id == c.claim_id && s.handle_key.0 == *hk)
                .then(|| (s.vault_id, s.locate.kdf.clone(), s.locate.auth_salt_mp, s.locate.auth_salt_rk))
        }))
    }

    /// `HMAC-SHA256(pepper, label ‖ handle_key)` truncated to 16 bytes.
    fn fake(&self, label: &[u8], hk: &[u8; 32]) -> [u8; 16] {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&self.cfg.pepper).expect("any key length");
        mac.update(label);
        mac.update(hk);
        let out = mac.finalize().into_bytes();
        let mut v = [0u8; 16];
        v.copy_from_slice(&out[..16]);
        v
    }
}
