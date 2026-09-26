//! Request authentication (spec v0.4 §11.4 "Provider verification") and
//! the replay cache. Any failure → no side effect beyond the MP-class
//! throttle slot a failed guess consumes (§11.5). Unknown vault, unknown
//! key and a bad signature are the same generic `401 AUTH_INVALID`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

use vault_proto::crypto::hex;
use vault_proto::crypto::recovery_auth::{key_id, RecoveryClass, CLASS_DEVICE};
use vault_proto::errors::ErrorCode;
use vault_proto::request::{self, Operation, ProviderRequest};

use crate::model::VaultStateDoc;
use crate::{throttle, Provider};

/// Allowed `|now − t|` (§11.4).
pub const CLOCK_WINDOW: u64 = 300;
/// Per-(vault, key) read-nonce cache size (§11.4: ≥ 10 000, LRU).
const READ_NONCES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signer {
    Device([u8; 16]),
    Recovery(RecoveryClass),
}

pub struct Authenticated {
    pub req: ProviderRequest,
    pub signer: Signer,
}

/// What the route handler already knows about the request.
pub struct Incoming<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub vault_id: [u8; 16],
    pub operation: Operation,
    pub auth: Option<&'a str>,
    pub body: &'a [u8],
    pub now: u64,
}

/// The signer of a `create`: the genesis device of the supplied registry.
pub struct GenesisSigner {
    pub device_id: [u8; 16],
    pub sign_pub: [u8; 65],
}

/// Per-(vault, key id) nonces in arrival order plus a membership set.
type NonceLru = HashMap<[u8; 48], (VecDeque<[u8; 16]>, HashSet<[u8; 16]>)>;

#[derive(Default)]
pub struct ReadNonces {
    inner: Mutex<NonceLru>,
}

impl ReadNonces {
    /// `false` if `(vid, key_id, n)` was already seen by this instance.
    fn insert(&self, vid: &[u8; 16], kid: &[u8; 32], n: [u8; 16]) -> bool {
        let mut k = [0u8; 48];
        k[..16].copy_from_slice(vid);
        k[16..].copy_from_slice(kid);
        let mut m = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let (order, set) = m.entry(k).or_default();
        if !set.insert(n) {
            return false;
        }
        order.push_back(n);
        if order.len() > READ_NONCES {
            if let Some(old) = order.pop_front() {
                set.remove(&old);
            }
        }
        true
    }
}

impl Provider {
    pub(crate) fn authenticate(
        &self,
        inc: &Incoming<'_>,
        state: Option<&VaultStateDoc>,
        genesis: Option<&GenesisSigner>,
    ) -> Result<Authenticated, ErrorCode> {
        let (req, tlv, sig) = request::parse_auth_header(inc.auth.ok_or(ErrorCode::AuthInvalid)?)?;
        // The MP-class throttle runs before anything is verified (§11.5),
        // keyed by the vault the request names — real or not.
        let slot = if req.signer_class == RecoveryClass::Mp.code() {
            Some(throttle::reserve(self, &req.vault_id, inc.now)?)
        } else {
            None
        };
        let verified = self.verify(inc, &req, &tlv, &sig, state, genesis);
        if let (Some(slot), Ok(_)) = (&slot, &verified) {
            throttle::release(self, slot);
        }
        let signer = verified?;
        let kid = req.signer_key_id;
        if req.operation.is_mutating() {
            let key = format!("v2/nonces/{}/{}/{}", hex::encode(req.vault_id), hex::encode(kid), hex::encode(req.n));
            let created = self.ops.create(&key, b"").map_err(|_| ErrorCode::BackupUnavailable)?;
            if created.is_none() {
                return Err(ErrorCode::BackupReplay);
            }
        } else if !self.read_nonces.insert(&req.vault_id, &kid, req.n) {
            return Err(ErrorCode::BackupReplay);
        }
        Ok(Authenticated { req, signer })
    }

    fn verify(
        &self,
        inc: &Incoming<'_>,
        req: &ProviderRequest,
        tlv: &[u8],
        sig: &[u8; 64],
        state: Option<&VaultStateDoc>,
        genesis: Option<&GenesisSigner>,
    ) -> Result<Signer, ErrorCode> {
        let bad = Err(ErrorCode::AuthInvalid);
        let bound = req.audience == self.cfg.origin
            && req.method == inc.method
            && req.path == inc.path
            && req.vault_id == inc.vault_id
            && req.operation == inc.operation
            && inc.now.abs_diff(req.t) <= CLOCK_WINDOW
            && req.body_sha256 == request::body_hash(inc.body);
        if !bound {
            return bad;
        }
        let (signer, public) = if req.signer_class == CLASS_DEVICE {
            let id = req.signer_device_id.ok_or(ErrorCode::AuthInvalid)?;
            let public = match (state, genesis) {
                (Some(s), _) => s.active(&id).map(|d| d.sign_pub),
                (None, Some(g)) if g.device_id == id => Some(g.sign_pub),
                _ => None,
            };
            (Signer::Device(id), public)
        } else {
            let class = RecoveryClass::from_code(req.signer_class).ok_or(ErrorCode::AuthInvalid)?;
            (Signer::Recovery(class), state.and_then(|s| s.recovery_auth.get(class)).map(|k| k.public))
        };
        let Some(public) = public else { return bad };
        if key_id(&public) != req.signer_key_id {
            return bad;
        }
        request::verify_signature(tlv, sig, &public)?;
        Ok(signer)
    }
}
