//! Provider-internal documents (spec v0.4 §11.2 state object, §11.3.1
//! claim object) and the transport-neutral response type.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use vault_proto::crypto::recovery_auth::RecoveryClass;
use vault_proto::errors::ErrorCode;
use vault_proto::header::{Hex16, Hex32, KdfBlock};
use vault_proto::state::{recovery_auth_digest, state_commit, RecoveryAuthEntry};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveDevice {
    pub device_id: Hex16,
    #[serde(with = "hex65")]
    pub sign_pub: [u8; 65],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthKey {
    #[serde(rename = "pub", with = "hex65")]
    pub public: [u8; 65],
    pub salt: Hex16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryAuth {
    pub mp: Option<AuthKey>,
    pub rk: Option<AuthKey>,
}

impl RecoveryAuth {
    pub fn get(&self, c: RecoveryClass) -> Option<&AuthKey> {
        match c {
            RecoveryClass::Mp => self.mp.as_ref(),
            RecoveryClass::Rk => self.rk.as_ref(),
        }
    }

    pub fn entries(&self) -> Vec<RecoveryAuthEntry> {
        let mut v = Vec::new();
        for (class, k) in [(RecoveryClass::Mp, &self.mp), (RecoveryClass::Rk, &self.rk)] {
            if let Some(k) = k {
                v.push(RecoveryAuthEntry { class, public: k.public, salt: k.salt.0 });
            }
        }
        v
    }

    pub fn apply(&mut self, updates: &[RecoveryAuthEntry]) {
        for u in updates {
            let k = Some(AuthKey { public: u.public, salt: Hex16(u.salt) });
            match u.class {
                RecoveryClass::Mp => self.mp = k,
                RecoveryClass::Rk => self.rk = k,
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Locate {
    pub kdf: KdfBlock,
    pub auth_salt_mp: Hex16,
    pub auth_salt_rk: Hex16,
}

/// Hashes of one committed state (current or retained).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateRef {
    pub generation: u64,
    pub manifest_hash: Hex32,
    pub checkpoint_hash: Hex32,
    pub index_hash: Hex32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitResult {
    pub generation: u64,
    pub state_commit: Hex32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recent {
    pub body_sha256: Hex32,
    pub result: CommitResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultStateDoc {
    pub v: u32,
    pub vault_id: Hex16,
    pub generation: u64,
    pub manifest_hash: Hex32,
    pub checkpoint_hash: Hex32,
    pub index_hash: Hex32,
    pub registry_hash: Hex32,
    pub registry_head: Hex32,
    pub registry_seq: u64,
    pub epoch: u64,
    pub header_hash: Hex32,
    pub vk_generation: u32,
    pub active_devices: Vec<ActiveDevice>,
    pub recovery_auth: RecoveryAuth,
    pub locate: Locate,
    pub handle_key: Hex32,
    pub claim_id: Hex16,
    pub state_commit: Hex32,
    pub retained: Vec<StateRef>,
    pub recent: Vec<Recent>,
    /// expected (old) generation → the committed finalize body and result.
    pub finalized: std::collections::BTreeMap<String, Recent>,
}

impl VaultStateDoc {
    pub fn compute_commit(&self) -> Result<[u8; 32], ErrorCode> {
        let digest = recovery_auth_digest(&self.recovery_auth.entries())?;
        Ok(state_commit(&self.vault_id.0, self.generation, &self.manifest_hash.0, &self.checkpoint_hash.0, &digest))
    }

    pub fn current_ref(&self) -> StateRef {
        StateRef {
            generation: self.generation,
            manifest_hash: self.manifest_hash,
            checkpoint_hash: self.checkpoint_hash,
            index_hash: self.index_hash,
        }
    }

    pub fn active(&self, device_id: &[u8; 16]) -> Option<&ActiveDevice> {
        self.active_devices.iter().find(|d| &d.device_id.0 == device_id)
    }

    pub fn result(&self) -> CommitResult {
        CommitResult { generation: self.generation, state_commit: self.state_commit }
    }

    pub fn parse(bytes: &[u8]) -> Result<VaultStateDoc, ErrorCode> {
        serde_json::from_slice(bytes).map_err(|_| ErrorCode::Internal)
    }

    pub fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("state serializes")
    }
}

/// §11.3.1 claim object `v2/handles/{handle_key}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub vault_id: Hex16,
    pub claim_id: Hex16,
    pub created_at: u64,
    pub status: String,
}

/// Transport-neutral response: the in-process transport and the axum
/// adapter both render this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    /// `application/json` or `application/octet-stream`.
    pub json: bool,
}

impl Response {
    pub fn json(status: u16, v: Value) -> Response {
        Response { status, body: v.to_string().into_bytes(), json: true }
    }

    pub fn bytes(body: Vec<u8>) -> Response {
        Response { status: 200, body, json: false }
    }

    pub fn error(code: ErrorCode) -> Response {
        Response::error_with(code, json!({}))
    }

    pub fn error_with(code: ErrorCode, mut extra: Value) -> Response {
        extra["error"] = json!(code.as_str());
        Response::json(status_of(code), extra)
    }
}

/// A refusal with optional extra response fields (e.g. `BLOB_MISSING
/// {count}`, `STATE_MOVED {state_commit, generation}`).
#[derive(Debug, Clone, PartialEq)]
pub struct Reject(pub ErrorCode, pub Value);

impl From<ErrorCode> for Reject {
    fn from(c: ErrorCode) -> Reject {
        Reject(c, json!({}))
    }
}

impl From<Reject> for Response {
    fn from(r: Reject) -> Response {
        Response::error_with(r.0, r.1)
    }
}

/// §11.3 error table.
pub fn status_of(code: ErrorCode) -> u16 {
    match code {
        ErrorCode::AuthInvalid => 401,
        ErrorCode::DeviceNotAuthorized => 403,
        ErrorCode::NotFound => 404,
        ErrorCode::BackupReplay | ErrorCode::StateMoved | ErrorCode::HandleTaken => 409,
        ErrorCode::BlobMissing => 412,
        ErrorCode::TooLarge => 413,
        ErrorCode::RecoveryThrottled => 429,
        ErrorCode::BackupUnavailable => 503,
        ErrorCode::InvalidInput => 400,
        ErrorCode::Internal => 500,
        _ => 422,
    }
}

mod hex65 {
    use serde::{Deserialize, Deserializer, Serializer};
    use vault_proto::header::strict_hex;

    pub fn serialize<S: Serializer>(v: &[u8; 65], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&vault_proto::crypto::hex::encode(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 65], D::Error> {
        let s = String::deserialize(d)?;
        strict_hex::<65>(&s).ok_or_else(|| serde::de::Error::custom("bad hex65"))
    }
}
