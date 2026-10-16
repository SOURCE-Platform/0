//! The push relay (spec §7.2; Phase G design `phase-g-design.md`): the
//! only Source-infrastructure involvement in remote approval. A device of
//! the vault registers its APNs token; a device asks to wake another
//! device of the vault. Both are device-signed §11.4 requests (codes 32,
//! 33). The alert is fixed — generic title and body, the opaque request id
//! — so APNs sees an opaque UUID, generic text and timing; this service
//! sees the token, the UUID, timing and the vault id. Alert pushes only,
//! `apns-expiration` = now + 120 s. Nothing actionable travels: a spoofed
//! push can only open the app.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};
use vault_proto::crypto::hex;
use vault_proto::errors::ErrorCode;

use crate::auth::{Incoming, Signer};
use crate::model::{Reject, Response};
use crate::Provider;

/// §7.2: aligned with the approval challenge's lifetime.
pub const EXPIRATION_SECS: u64 = 120;
/// Wakes per vault per minute: enough for a person, too few to spam.
pub const SENDS_PER_MINUTE: usize = 10;
const MAX_TOKEN_HEX: usize = 200;

/// APNs, or a stand-in. `payload` is the complete JSON body.
pub trait PushSender: Send + Sync {
    fn send(&self, device_token: &str, payload: &str, expiration: u64) -> Result<(), String>;
}

/// The whole push payload (§7.2), nothing else.
pub fn payload(request_id: &str) -> String {
    json!({ "aps": { "alert": { "title": "Source Vault", "body": "Approval request — open Source to review" }, "sound": "default" }, "rid": request_id }).to_string()
}

#[derive(Default)]
pub struct PushLimits {
    sent: Mutex<HashMap<[u8; 16], Vec<u64>>>,
}

impl PushLimits {
    fn allow(&self, vid: &[u8; 16], now: u64) -> bool {
        let mut m = self.sent.lock().unwrap_or_else(|e| e.into_inner());
        let v = m.entry(*vid).or_default();
        v.retain(|t| now.saturating_sub(*t) < 60);
        if v.len() >= SENDS_PER_MINUTE {
            return false;
        }
        v.push(now);
        true
    }
}

fn token_key(vid: &[u8; 16], device: &[u8; 16]) -> String {
    format!("v2/push/{}/{}", hex::encode(vid), hex::encode(device))
}

fn body(inc: &Incoming<'_>) -> Result<Value, Reject> {
    Ok(serde_json::from_slice(inc.body).map_err(|_| ErrorCode::InvalidInput)?)
}

impl Provider {
    /// `POST /v2/vaults/{vid}/push/token {token}`: the signer's own token.
    pub(crate) fn push_register(&self, inc: &Incoming<'_>) -> Result<Response, Reject> {
        let (s, _) = self.load_state(&inc.vault_id)?.ok_or(ErrorCode::AuthInvalid)?;
        let a = self.authenticate(inc, Some(&s), None)?;
        let Signer::Device(me) = a.signer else { return Err(ErrorCode::AuthInvalid.into()) };
        let token = body(inc)?.get("token").and_then(Value::as_str).filter(|t| !t.is_empty() && t.len() <= MAX_TOKEN_HEX && hex::decode(t).is_some()).ok_or(ErrorCode::InvalidInput)?.to_string();
        let key = token_key(&inc.vault_id, &me);
        let _ = self.ops.delete(&key);
        self.ops.create(&key, token.as_bytes()).map_err(|_| ErrorCode::BackupUnavailable)?;
        Ok(Response::json(200, json!({ "ok": true })))
    }

    /// `POST /v2/vaults/{vid}/push/send {to, rid}`: wake an active device of
    /// the signer's vault with the fixed alert.
    pub(crate) fn push_send(&self, inc: &Incoming<'_>) -> Result<Response, Reject> {
        let (s, _) = self.load_state(&inc.vault_id)?.ok_or(ErrorCode::AuthInvalid)?;
        let a = self.authenticate(inc, Some(&s), None)?;
        if !matches!(a.signer, Signer::Device(_)) {
            return Err(ErrorCode::AuthInvalid.into());
        }
        let b = body(inc)?;
        let to = b.get("to").and_then(Value::as_str).and_then(hex::decode_array::<16>).ok_or(ErrorCode::InvalidInput)?;
        let rid = b.get("rid").and_then(Value::as_str).filter(|r| hex::decode_array::<16>(r).is_some()).ok_or(ErrorCode::InvalidInput)?;
        if s.active(&to).is_none() {
            return Err(ErrorCode::NotFound.into());
        }
        let sender = self.push.as_ref().ok_or(ErrorCode::BackupUnavailable)?;
        if !self.push_limits.allow(&inc.vault_id, inc.now) {
            return Err(ErrorCode::PeerLimit.into());
        }
        let token = self.ops.get(&token_key(&inc.vault_id, &to)).map_err(|_| ErrorCode::BackupUnavailable)?.ok_or(ErrorCode::NotFound)?.0;
        let token = String::from_utf8(token).map_err(|_| ErrorCode::BackupUnavailable)?;
        sender.send(&token, &payload(rid), inc.now + EXPIRATION_SECS).map_err(|_| ErrorCode::BackupUnavailable)?;
        Ok(Response::json(200, json!({ "ok": true })))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_payload_is_generic() {
        let p: serde_json::Value = serde_json::from_str(&super::payload("00112233445566778899aabbccddeeff")).unwrap();
        assert_eq!(p["aps"]["alert"]["body"], "Approval request — open Source to review");
        assert_eq!(p.as_object().unwrap().len(), 2, "aps and rid only");
    }
}
