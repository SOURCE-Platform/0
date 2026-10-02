//! Provider-work sessions and the shared §1.5 ops (spec v0.4 §1.3,
//! §11.4): `stream_read`, `stream_begin`/`write`/`end`/`cancel`,
//! `sign_provider_request`, `session_close`, `quarantine_status`. The
//! publication, sync and recovery ops live in `backup_ops`, `sync_ops`
//! and `recovery_flow`.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use vault_proto::b64;
use vault_proto::request::Operation;

use super::{lock_core, OpOutcome, VaultCore};
use crate::backup::index::ObjectIndex;
use crate::crypto::hex;
use crate::device::SeDevice;
use crate::errors::ErrorCode;
use crate::recovery::complete::Completed;
use crate::recovery::total_loss::{Preview, Recovery};
use crate::state::VaultState;
use crate::sync::publish::Staging;
use crate::sync::remote::RemoteState;
use crate::sync::session::{Id, Transfer};
use crate::sync::sign::{self, Key, SignRequest, SignScope};

pub struct PublishSession {
    pub t: Transfer,
    pub staging: Staging,
}

pub struct SyncSession {
    pub t: Transfer,
    pub remote: RemoteState,
    pub index: Option<ObjectIndex>,
}

pub struct RecoverySession {
    pub t: Transfer,
    pub rec: Recovery,
    pub remote: Option<RemoteState>,
    pub index: Option<ObjectIndex>,
    pub preview: Option<Preview>,
    pub completed: Option<Completed>,
    /// A newly issued RK was acknowledged on its sheet.
    pub acknowledged: bool,
    /// The normalized handle main located the vault by (display data for
    /// the sheet and the recovered vault's `kv`, like `setup_vault`'s).
    pub handle: Option<String>,
}

#[derive(Default)]
pub struct Sessions {
    pub publish: Option<PublishSession>,
    pub sync: Option<SyncSession>,
    pub recovery: Option<RecoverySession>,
    /// §1.3 TR-09: an enrollment bundle too large for one frame, read by
    /// main with `stream_read` (ciphertext and public data only).
    pub bundle: Option<Transfer>,
    /// Peer responses too large for one frame (wire annex A.2.2).
    pub peer: Vec<Transfer>,
    /// Peer requests whose large body is still streaming in: the
    /// authenticated request and the transfer receiving its body.
    pub peer_in: Vec<(crate::peer::verify::Accepted, Transfer)>,
}

/// Wire annex A.2.2: at most two peer sessions open, idle 60 s.
pub const MAX_PEER_SESSIONS: usize = 2;
pub const PEER_IDLE: std::time::Duration = std::time::Duration::from_secs(60);

impl Sessions {
    /// §13.3: lock aborts a sync and a recovery; a fully staged
    /// publication continues without the VK.
    pub fn on_lock(&mut self) {
        self.sync = None;
        self.recovery = None;
        self.bundle = None; // the enrollment it belongs to dies with the lock
        self.peer.clear(); // lock aborts a peer session (annex A.2.2)
        self.peer_in.clear();
    }

    pub fn peer_sessions(&self) -> usize {
        self.peer.len() + self.peer_in.len()
    }

    pub fn transfer(&mut self, id: &Id) -> Option<&mut Transfer> {
        if let Some(p) = self.publish.as_mut().filter(|p| &p.t.id == id) {
            return Some(&mut p.t);
        }
        if let Some(s) = self.sync.as_mut().filter(|s| &s.t.id == id) {
            return Some(&mut s.t);
        }
        if let Some(b) = self.bundle.as_mut().filter(|b| &b.id == id) {
            return Some(b);
        }
        if let Some(p) = self.peer.iter_mut().find(|p| &p.id == id) {
            return Some(p);
        }
        if let Some((_, t)) = self.peer_in.iter_mut().find(|(_, t)| &t.id == id) {
            return Some(t);
        }
        self.recovery.as_mut().filter(|r| &r.t.id == id).map(|r| &mut r.t)
    }

    /// §1.3 idle limits. A fully staged publication is exempt: it is
    /// resumed by the next cycle (§11.3.2), including while LOCKED.
    pub fn expire(&mut self) {
        self.sync = self.sync.take().filter(|s| !s.t.expired());
        self.bundle = self.bundle.take().filter(|b| !b.expired());
        self.peer.retain(|p| !p.idle_longer_than(PEER_IDLE));
        self.peer_in.retain(|(_, t)| !t.idle_longer_than(PEER_IDLE));
        self.recovery = self.recovery.take().filter(|r| !r.t.expired());
    }
}

impl VaultCore {
    /// The state reported to clients: provider work shows as BACKING_UP /
    /// SYNCING over an UNLOCKED (or, for a staged publication, LOCKED)
    /// vault, which keeps serving every op it otherwise would (§13.2).
    pub fn reported_state(&self) -> VaultState {
        match self.state {
            VaultState::Unlocked if self.provider.sync.is_some() => VaultState::Syncing,
            VaultState::Unlocked | VaultState::Locked if self.provider.publish.is_some() => VaultState::BackingUp,
            s => s,
        }
    }
}

pub fn session_id(frame: &Value) -> Result<Id, ErrorCode> {
    frame.get("session").and_then(Value::as_str).and_then(hex::decode_array::<16>).ok_or(ErrorCode::InvalidInput)
}

fn sha_param(frame: &Value, key: &str) -> Result<[u8; 32], ErrorCode> {
    frame.get(key).and_then(Value::as_str).and_then(hex::decode_array::<32>).ok_or(ErrorCode::InvalidInput)
}

/// `stream_read {session, sha256, offset}` → `{data, offset, total_len, eof}`.
pub fn stream_read(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    let run = || -> Result<Value, ErrorCode> {
        let (id, sha) = (session_id(frame)?, sha_param(frame, "sha256")?);
        let offset = frame.get("offset").and_then(Value::as_u64).ok_or(ErrorCode::InvalidInput)?;
        let mut c = lock_core(core);
        let t = c.provider.transfer(&id).ok_or(ErrorCode::TransferInvalid)?;
        let (data, total, eof) = t.read(&sha, offset)?;
        Ok(json!({ "data": b64::encode(&data), "offset": offset, "total_len": total, "eof": eof }))
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}

/// `stream_begin {session, sha256, size}` → `{stream_id}`.
pub fn stream_begin(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    let run = || -> Result<Value, ErrorCode> {
        let (id, sha) = (session_id(frame)?, sha_param(frame, "sha256")?);
        let size = frame.get("size").and_then(Value::as_u64).ok_or(ErrorCode::InvalidInput)?;
        let mut c = lock_core(core);
        let t = c.provider.transfer(&id).ok_or(ErrorCode::TransferInvalid)?;
        Ok(json!({ "stream_id": hex::encode(t.begin(sha, size)?) }))
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}

/// `stream_write {session, stream_id, seq, offset, data}`, `stream_end`,
/// `stream_cancel` (`op` selects).
pub fn stream_io(core: &Arc<Mutex<VaultCore>>, frame: &Value, op: &str) -> OpOutcome {
    let run = || -> Result<Value, ErrorCode> {
        let id = session_id(frame)?;
        let stream = frame.get("stream_id").and_then(Value::as_str).and_then(hex::decode_array::<16>).ok_or(ErrorCode::InvalidInput)?;
        let mut c = lock_core(core);
        let t = c.provider.transfer(&id).ok_or(ErrorCode::TransferInvalid)?;
        match op {
            "stream_write" => {
                let seq = frame.get("seq").and_then(Value::as_u64).ok_or(ErrorCode::InvalidInput)?;
                let offset = frame.get("offset").and_then(Value::as_u64).ok_or(ErrorCode::InvalidInput)?;
                let data = frame.get("data").and_then(Value::as_str).and_then(b64::decode).ok_or(ErrorCode::TransferInvalid)?;
                t.write(&stream, seq, offset, &data)?;
                Ok(json!({}))
            }
            "stream_end" => Ok(json!({ "sha256": hex::encode(t.end(&stream)?) })),
            _ => {
                t.cancel(&stream);
                Ok(json!({}))
            }
        }
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}

/// `session_close {session}`: the session and its staging are dropped.
pub fn session_close(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &super::Deps) -> OpOutcome {
    let Ok(id) = session_id(frame) else { return OpOutcome::err(ErrorCode::InvalidInput) };
    let mut c = lock_core(core);
    let p = &mut c.provider;
    if p.bundle.as_ref().is_some_and(|b| b.id == id) {
        p.bundle = None;
    } else if p.publish.as_ref().is_some_and(|s| s.t.id == id) {
        p.publish = None;
    } else if p.sync.as_ref().is_some_and(|s| s.t.id == id) {
        p.sync = None;
    } else if p.peer.iter().any(|t| t.id == id) || p.peer_in.iter().any(|(_, t)| t.id == id) {
        p.peer.retain(|t| t.id != id);
        p.peer_in.retain(|(_, t)| t.id != id);
    } else if p.recovery.as_ref().is_some_and(|s| s.t.id == id) {
        if let Some(ev) = c.leave_recovery() {
            deps.events.emit(ev);
        }
    }
    OpOutcome::ok(json!({}))
}

/// Persisted COMPROMISED evidence, from the open store or the directory.
fn evidence_on_file(c: &VaultCore) -> Result<bool, ErrorCode> {
    let opened;
    let store = match c.store.as_ref() {
        Some(s) => s,
        None => {
            opened = crate::storage::VaultStore::open(&c.vault_dir).map_err(|_| ErrorCode::SigningRefused)?;
            &opened
        }
    };
    Ok(crate::storage::compromised::load(&store.conn).map_err(|_| ErrorCode::SigningRefused)?.is_some())
}

/// `quarantine_status`: refused-revision counts per reason (§3.2).
pub fn quarantine_status(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    let c = lock_core(core);
    let Some(store) = c.store.as_ref().filter(|_| c.state == VaultState::Unlocked) else {
        return OpOutcome::err(ErrorCode::BadState);
    };
    match crate::storage::rev_state::refused_totals(&store.conn) {
        Ok(t) => OpOutcome::ok(json!({ "refused": t.iter().map(|(r, n)| json!({"reason": r, "count": n})).collect::<Vec<_>>() })),
        Err(e) => OpOutcome::err(e),
    }
}

fn operation(frame: &Value) -> Result<Operation, ErrorCode> {
    match frame.get("operation").and_then(Value::as_str) {
        Some("state_get") => Ok(Operation::StateGet),
        Some("blob_get") => Ok(Operation::BlobGet),
        Some("blob_put") => Ok(Operation::BlobPut),
        Some("state_commit") => Ok(Operation::StateCommit),
        _ => Err(ErrorCode::SigningRefused),
    }
}

/// `sign_provider_request {session?, operation, sha256?, body_sha256,
/// expected_state?}` → `{auth, method, path}`. The helper builds every
/// canonical field and checks the §11.4 state × operation × class table:
/// no signature over anything main could substitute (PR-02, PR-05, PR-06).
pub fn sign_provider_request(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    let run = || -> Result<Value, ErrorCode> {
        let op = operation(frame)?;
        let blob = frame.get("sha256").and_then(Value::as_str).and_then(hex::decode_array::<32>);
        let body_sha256 = sha_param(frame, "body_sha256").map_err(|_| ErrorCode::SigningRefused)?;
        let expected_state = frame.get("expected_state").and_then(Value::as_str).and_then(hex::decode_array::<32>);
        let req = SignRequest { operation: op, blob, body_sha256, expected_state };
        let now = crate::storage::store::now_epoch();
        let c = lock_core(core);
        let none = BTreeSet::new();
        let reads_only = SignScope { put_blobs: &none, staged: None };
        let (pr, header) = match c.state {
            VaultState::Recovering => {
                let r = c.provider.recovery.as_ref().ok_or(ErrorCode::SigningRefused)?;
                let put: BTreeSet<[u8; 32]>;
                let scope = match &r.completed {
                    Some(done) if r.acknowledged => {
                        put = done.staging.blobs.keys().copied().collect();
                        SignScope { put_blobs: &put, staged: Some((done.staging.body_sha256, done.staging.expected_state)) }
                    }
                    _ => SignScope { put_blobs: &none, staged: None },
                };
                let h = r.rec.sign(&req, &scope, now)?;
                let (m, p) = op.route(&r.rec.locate.vault_id, blob.as_ref()).ok_or(ErrorCode::SigningRefused)?;
                (json!({ "method": m, "path": p, "origin": r.rec.origin }), h)
            }
            VaultState::Locked | VaultState::Unlocked | VaultState::Compromised => {
                let header = c.store.as_ref().map(|s| s.header.clone()).or(c.header.clone()).ok_or(ErrorCode::SigningRefused)?;
                let me = SeDevice::load(&c.vault_dir).map_err(|_| ErrorCode::SigningRefused)?;
                let put: BTreeSet<[u8; 32]>;
                // §13.2: COMPROMISED (or its evidence, while LOCKED) reads
                // only — no write is ever signed over fork evidence.
                let frozen = c.state == VaultState::Compromised || (c.provider.publish.is_some() && evidence_on_file(&c)?);
                let scope = match &c.provider.publish {
                    // LOCKED: writes only for a fully staged publication.
                    Some(p) if !frozen => {
                        put = p.staging.blobs.keys().copied().collect();
                        SignScope { put_blobs: &put, staged: Some((p.staging.body_sha256, p.staging.expected_state)) }
                    }
                    _ => reads_only,
                };
                let (pr, h) = sign::sign(&header.provider, header.vault_id.0, Key::Device(&me), &req, &scope, 0, now)?;
                (json!({ "method": pr.method, "path": pr.path, "origin": header.provider }), h)
            }
            _ => return Err(ErrorCode::SigningRefused),
        };
        let mut out = pr;
        out["auth"] = json!(header);
        Ok(out)
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}
