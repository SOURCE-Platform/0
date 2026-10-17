//! Reverse enrollment, the joining Mac's side (spec v0.5 §22.13; design
//! `phase-f2e-design.md` revision 2, §3–§5). UNINITIALIZED only. The
//! attempt lives in a staging directory (`renroll/`) — its own identity,
//! later its vault — and moves into the vault root only at `renroll_finish`,
//! after the iPhone confirmed it committed the entry. Nothing of an
//! existing vault is ever touched (review SEC-I2).
//!
//! - `renroll_begin {name?}` → `{secret, commit, expires_in}`: fresh keys,
//!   secret and `nonce_n`; the nonce and keys stay here until the hello
//!   (reviews SEC-I1 / SPEC-I2). Six begins per minute, one at a time.
//! - `renroll_hello {hello}` → the reply that opens the commitment; the
//!   transcript and code are fixed. A second correct hello ends it.
//! - `renroll_proof {proof}` → the bundle and `done` routes' check.
//! - `renroll_confirm` → the "Join a Vault" panel: the code and the vault's
//!   master password (which must later open the bundle's wrap).
//! - `renroll_bundle_begin {sha256, size}` → a stream session, only after
//!   the panel's Continue. `renroll_complete`, `renroll_finish` and
//!   `renroll_abort` are in `renroll_commit.rs`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value};
use subtle::ConstantTimeEq;

use super::{lock_core, Deps, OpOutcome, PanelOutcome, PanelRequest, VaultCore};
use crate::crypto::hex;
use crate::crypto::secret::SecretVec;
use crate::device::identity::{self, SeDevice};
use crate::enroll::reverse::{self, Hello, Reply};
use crate::enroll::session::{MAX_FAILURES, SESSION_TTL};
use crate::enroll::transcript::{decode_secret, encode_secret, sas};
use crate::errors::ErrorCode;
use crate::registry::device::{DeviceIdentity, PLATFORM_MACOS};
use crate::state::VaultState;
use crate::sync::session::Transfer;

pub const STAGING: &str = "renroll";
pub const MAX_BUNDLE: u64 = 256 << 20;

/// What the hello fixed.
pub struct Fixed {
    pub nonce_e: [u8; 16],
    pub authorizer_device_id: [u8; 16],
    pub authorizer_key: [u8; 32],
    pub new_device_id: [u8; 16],
    pub vault_id: [u8; 16],
    pub sas: String,
}

pub struct RenrollSession {
    pub fp: [u8; 32],
    secret: [u8; 16],
    nonce_n: [u8; 16],
    pub name: String,
    pub started: Instant,
    pub failures: u32,
    pub fixed: Option<Fixed>,
    /// The panel's Continue, with the master password it collected.
    pub mp: Option<SecretVec>,
    pub transfer: Option<Transfer>,
    /// The registry head ACKed, waiting for the iPhone's `done`.
    pub acked: Option<[u8; 32]>,
    /// The Keychain tag of the keys this attempt created: the only keys
    /// its cleanup may delete (final review, core 1).
    pub key_tag: String,
}

impl RenrollSession {
    pub fn expired(&self) -> bool {
        self.started.elapsed() >= SESSION_TTL
    }
    pub fn secret(&self) -> &[u8; 16] {
        &self.secret
    }
}

impl Drop for RenrollSession {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.secret.zeroize();
        self.nonce_n.zeroize();
    }
}

pub fn staging(vault_dir: &Path) -> PathBuf {
    vault_dir.join(STAGING)
}

/// Remove an attempt's staging directory and the keys it created.
///
/// The keys go only by `tag` — the attempt's own, kept in its session —
/// never by a tag read back from disk while a vault exists here (a
/// same-user process could plant one; final review, core 1). Without a
/// session (helper start), the staging's own `device.json` names them, and
/// only when no vault exists. The vault root's own keys are never deleted.
pub fn wipe_staging(vault_dir: &Path, tag: Option<&str>) {
    let dir = staging(vault_dir);
    if !dir.exists() {
        return;
    }
    let no_vault = !vault_dir.join(crate::VAULT_HEADER_NAME).exists();
    let tag = tag.map(String::from).or_else(|| no_vault.then(|| identity::read_file(&dir).ok().map(|f| f.key_tag)).flatten());
    let root_tag = identity::read_file(vault_dir).ok().map(|f| f.key_tag);
    if let Some(t) = tag.filter(|t| Some(t) != root_tag.as_ref()) {
        crate::device::se::delete_keys(&t);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// At helper start: an attempt left behind, only where no vault exists —
/// including what an interrupted `renroll_finish` already moved into the
/// root (it moves the header last, so no header means no vault).
pub fn recover_at_boot(vault_dir: &Path) {
    if vault_dir.join(crate::VAULT_HEADER_NAME).exists() || !staging(vault_dir).exists() {
        return;
    }
    wipe_staging(vault_dir, None);
    identity::wipe(vault_dir);
    super::create::cleanup_partial_vault(vault_dir);
}

fn run(f: impl FnOnce() -> Result<Value, ErrorCode>) -> OpOutcome {
    f().map_or_else(OpOutcome::err, OpOutcome::ok)
}

fn joinable(c: &VaultCore) -> Result<(), ErrorCode> {
    if c.phone || c.state != VaultState::Uninitialized || c.vault_dir.join(crate::VAULT_HEADER_NAME).exists() {
        return Err(ErrorCode::BadState);
    }
    Ok(())
}

pub fn renroll_begin(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let fp = frame.get("fp").and_then(Value::as_str).and_then(hex::decode_array::<32>).ok_or(ErrorCode::InvalidInput)?;
        let name = frame.get("name").and_then(Value::as_str).map_or_else(|| "Mac".to_string(), vault_proto::crypto::registry::clamp_device_name);
        let dir = {
            let c = lock_core(core);
            joinable(&c)?;
            if c.provider.renroll.as_ref().is_some_and(|s| !s.expired()) {
                return Err(ErrorCode::BadState);
            }
            c.vault_dir.clone()
        };
        if !super::enroll_ops::begin_allowed_pub() {
            return Err(ErrorCode::BadState); // at most a few per minute, as forward
        }
        wipe_staging(&dir, None);
        std::fs::create_dir_all(staging(&dir)).map_err(|_| ErrorCode::Internal)?;
        let me = SeDevice::create(&staging(&dir), &name, PLATFORM_MACOS)?;
        let key_tag = identity::read_file(&staging(&dir))?.key_tag;
        let (mut secret, mut nonce_n) = ([0u8; 16], [0u8; 16]);
        getrandom::fill(&mut secret).map_err(|_| ErrorCode::Internal)?;
        getrandom::fill(&mut nonce_n).map_err(|_| ErrorCode::Internal)?;
        let commit = reverse::commitment(&nonce_n, &me.sign_pub(), &me.agree_pub(), &name, PLATFORM_MACOS);
        let session = RenrollSession { fp, secret, nonce_n, name, started: Instant::now(), failures: 0, fixed: None, mp: None, transfer: None, acked: None, key_tag };
        let out = json!({ "secret": encode_secret(session.secret()), "commit": hex::encode(commit), "expires_in": SESSION_TTL.as_secs() });
        lock_core(core).provider.renroll = Some(session);
        Ok(out)
    })
}

fn end(core: &Arc<Mutex<VaultCore>>) {
    let mut c = lock_core(core);
    if let Some(s) = c.provider.renroll.take() {
        wipe_staging(&c.vault_dir, Some(&s.key_tag));
    }
}

pub fn renroll_hello(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    let hello: Hello = match frame.get("hello").cloned().map(serde_json::from_value) {
        Some(Ok(h)) => h,
        _ => return OpOutcome::err(ErrorCode::InvalidInput),
    };
    let result = (|| {
        let mut c = lock_core(core);
        let dir = c.vault_dir.clone();
        let s = c.provider.renroll.as_mut().filter(|s| !s.expired()).ok_or((ErrorCode::BadState, false))?;
        let presented = decode_secret(&hello.secret).unwrap_or([0; 16]);
        if !bool::from(s.secret.ct_eq(&presented)) || hello.v != reverse::PROTO_V3 {
            s.failures += 1;
            return Err((ErrorCode::WrongCredential, s.failures >= MAX_FAILURES));
        }
        if s.fixed.is_some() {
            return Err((ErrorCode::ProtocolViolation, true)); // two devices saw the QR (review SEC-O2)
        }
        let bad = || (ErrorCode::InvalidInput, false);
        let nonce_e = hex::decode_array::<16>(&hello.nonce_e).ok_or_else(bad)?;
        let authorizer_device_id = hex::decode_array::<16>(&hello.authorizer_device_id).ok_or_else(bad)?;
        let authorizer_sign_pub = hex::decode_array::<65>(&hello.authorizer_sign_pub).ok_or_else(bad)?;
        let new_device_id = hex::decode_array::<16>(&hello.new_device_id).ok_or_else(bad)?;
        let vault_id = hex::decode_array::<16>(&hello.vault_id).ok_or_else(bad)?;
        let mut me = SeDevice::load(&staging(&dir)).map_err(|e| (e, true))?;
        me.adopt_id(&staging(&dir), new_device_id).map_err(|e| (e, true))?;
        let authorizer_key = reverse::authorizer_key(&authorizer_sign_pub);
        let t = reverse::transcript(&reverse::Binding {
            fp: &s.fp,
            authorizer_key: &authorizer_key,
            secret: &s.secret,
            nonce_e: &nonce_e,
            nonce_n: &s.nonce_n,
            authorizer_device_id: &authorizer_device_id,
            new_device_id: &new_device_id,
            sign_pub: &me.sign_pub(),
            agree_pub: &me.agree_pub(),
            vault_id: &vault_id,
        });
        let code = sas(&t).map_err(|e| (e, true))?;
        s.fixed = Some(Fixed { nonce_e, authorizer_device_id, authorizer_key, new_device_id, vault_id, sas: code });
        let reply = Reply { nonce_n: hex::encode(s.nonce_n), sign_pub: hex::encode(me.sign_pub()), agree_pub: hex::encode(me.agree_pub()), name: s.name.clone(), platform: PLATFORM_MACOS };
        serde_json::to_value(reply).map_err(|_| (ErrorCode::Internal, true))
    })();
    match result {
        Ok(v) => OpOutcome::ok(json!({ "reply": v })),
        Err((e, ends)) => {
            if ends {
                end(core);
            }
            OpOutcome::err(e)
        }
    }
}

pub fn renroll_proof(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    let proof = frame.get("proof").and_then(Value::as_str).and_then(hex::decode_array::<32>).unwrap_or([0; 32]);
    let c = lock_core(core);
    let ok = c.provider.renroll.as_ref().filter(|s| !s.expired()).and_then(|s| s.fixed.as_ref().map(|f| reverse::route_proof(&s.secret, &f.nonce_e))).is_some_and(|p| bool::from(p.ct_eq(&proof)));
    if ok { OpOutcome::ok(json!({})) } else { OpOutcome::err(ErrorCode::WrongCredential) }
}

/// The helper's own panel: the code, and the vault's master password. A
/// cancel is the Mac's "codes don't match": the attempt ends.
pub fn renroll_confirm(core: &Arc<Mutex<VaultCore>>, deps: &Deps) -> OpOutcome {
    use super::setup::{emit_panel, PANEL_TIMEOUT_PUB};
    let code = {
        let c = lock_core(core);
        match c.provider.renroll.as_ref().filter(|s| !s.expired() && s.mp.is_none()).and_then(|s| s.fixed.as_ref()) {
            Some(f) => f.sas.clone(),
            None => return OpOutcome::err(ErrorCode::BadState),
        }
    };
    emit_panel(deps, true, PanelRequest::JoinConfirm);
    let outcome = deps.panel.run_with_code(PanelRequest::JoinConfirm, &code, PANEL_TIMEOUT_PUB);
    emit_panel(deps, false, PanelRequest::JoinConfirm);
    let PanelOutcome::Submitted(mp) = outcome else {
        end(core);
        return OpOutcome::err(ErrorCode::PanelCancelled);
    };
    match lock_core(core).provider.renroll.as_mut().filter(|s| !s.expired()) {
        Some(s) => {
            s.mp = Some(mp);
            OpOutcome::ok(json!({}))
        }
        None => OpOutcome::err(ErrorCode::BadState),
    }
}

pub fn renroll_bundle_begin(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let sha = frame.get("sha256").and_then(Value::as_str).and_then(hex::decode_array::<32>).ok_or(ErrorCode::InvalidInput)?;
        let size = frame.get("size").and_then(Value::as_u64).filter(|n| *n <= MAX_BUNDLE).ok_or(ErrorCode::InvalidInput)?;
        let mut c = lock_core(core);
        let s = c.provider.renroll.as_mut().filter(|s| !s.expired() && s.mp.is_some() && s.acked.is_none()).ok_or(ErrorCode::BadState)?;
        let mut t = Transfer::new(Default::default());
        t.expect([(sha, size)]);
        let id = t.id;
        s.transfer = Some(t);
        Ok(json!({ "session": hex::encode(id) }))
    })
}
