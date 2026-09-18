//! Lifecycle ops (§1.5): `setup_vault`, `begin_recovery_unlock`,
//! `change_master_password`. MP bytes enter only via the helper's own
//! secure panel (§1.7), cross exactly one trust hop (derive → use →
//! zeroize, §2.11), and never appear in any IPC frame, log, or error.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use super::{
    ev_panel, ev_state, lock_core, Deps, OpOutcome, PanelOutcome, PanelRequest, VaultCore,
};
use crate::crypto::kdf::{self, Argon2Params};
use crate::crypto::secret;
use crate::crypto::wrap::{self, PasswordWrapFile, RecoveryWrapPayload};
use crate::errors::ErrorCode;
use crate::state::VaultState;
use crate::storage::header::{Header, Hex16};
use crate::storage::store::{write_atomic, PASSWORD_WRAP_NAME};
use crate::storage::VaultStore;
use crate::keychain;
use crate::VAULT_HEADER_NAME;

/// §13.3: UNLOCKING aborts after 120 s (also bounds panel waits).
const PANEL_TIMEOUT: Duration = Duration::from_secs(120);

/// Helper-panel window title prefix (§14.2: the main app registers this
/// title in the capture-exclusion registry on `secure_panel_visible`).
pub const PANEL_TITLE: &str = "Source Vault";

pub(super) fn emit_panel(deps: &Deps, visible: bool, req: PanelRequest) {
    deps.events.emit(ev_panel(visible, visible.then(|| req.title())));
}

/// Remove exactly the files `setup_vault` may have created, when a later
/// step fails. Never touches helper.sock (the live IPC endpoint shares
/// the directory) and never recurses beyond the known Phase C set.
fn cleanup_partial_vault(dir: &std::path::Path) {
    use crate::storage::{db::DB_NAME, manifest, store};
    for name in [
        VAULT_HEADER_NAME,
        DB_NAME,
        "vault.db-wal",
        "vault.db-shm",
        manifest::MANIFEST_NAME,
        crate::VAULT_REGISTRY_NAME,
        store::PASSWORD_WRAP_NAME,
    ] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    let _ = std::fs::remove_dir(dir.join(store::WRAPS_DIR));
    let _ = std::fs::remove_dir(dir.join(store::IMPORT_DIR));
}

/// §5.4 Phase C subset: VK + vault_id + MP wrap + empty registry + first
/// manifest (RK wrap/display is Phase D; the genesis registry entry is
/// Phase E — no device key exists yet). Ends LOCKED per §13.1; VK is
/// never installed by this op.
pub fn setup_vault(core: &Arc<Mutex<VaultCore>>, deps: &Deps) -> OpOutcome {
    {
        let c = lock_core(core);
        if c.state != VaultState::Uninitialized {
            return OpOutcome::err(ErrorCode::BadState);
        }
    }
    emit_panel(deps, true, PanelRequest::MpCreate);
    let outcome = deps.panel.run(PanelRequest::MpCreate, PANEL_TIMEOUT);
    emit_panel(deps, false, PanelRequest::MpCreate);
    let PanelOutcome::Submitted(mp) = outcome else {
        return OpOutcome::err(ErrorCode::PanelCancelled);
    };
    let vault_dir = lock_core(core).vault_dir.clone();
    let result = create_vault_files(&vault_dir, &mp);
    drop(mp); // SecretVec zeroizes
    let (header, wrap_json) = match result {
        Ok(v) => v,
        Err(e) => {
            cleanup_partial_vault(&vault_dir);
            return OpOutcome::err(e);
        }
    };
    if write_atomic(&vault_dir.join(PASSWORD_WRAP_NAME), &wrap_json).is_err() {
        cleanup_partial_vault(&vault_dir);
        return OpOutcome::err(ErrorCode::Internal);
    }
    // Rollback-evidence bookkeeping (§2.8). Keychain failure is not fatal
    // to creation; the vault opens without it (first-seen semantics).
    let _ = keychain::write_seen_generation(header.manifest_generation);
    let mut c = lock_core(core);
    c.header = Some(header);
    c.state = VaultState::Locked;
    deps.events.emit(ev_state(VaultState::Locked));
    OpOutcome::ok(json!({"state": "locked"}))
}

/// Build all vault bytes before touching the directory: header, DB schema,
/// manifest, registry, and the sealed MP wrap. Returns the header plus the
/// wrap file JSON (written by the caller last).
fn create_vault_files(
    vault_dir: &std::path::Path,
    mp: &[u8],
) -> Result<(Header, Vec<u8>), ErrorCode> {
    let vault_id = Hex16::random();
    let vk = secret::random_secret().mlock_best_effort();
    let header = Header::fresh(vault_id);
    let salt = header.kdf.salt_bytes()?;
    let pk = kdf::derive_pk(mp, &salt, header.kdf.params()).map_err(|_| ErrorCode::Internal)?;
    let payload = RecoveryWrapPayload {
        vk,
        wrapped_at: crate::storage::store::now_epoch(),
        vk_generation: 1,
    };
    let wrap_file = wrap::seal_wrap_mp(&payload, &pk, &vault_id.0, Argon2Params::V1, &salt)
        .map_err(|_| ErrorCode::Internal)?;
    drop(pk);
    let wrap_json = serde_json::to_vec_pretty(&wrap_file).map_err(|_| ErrorCode::Internal)?;
    VaultStore::create(vault_dir, header.clone())?;
    Ok((header, wrap_json))
}

/// §1.5 `begin_recovery_unlock` with `kind:"mp"`: the panel-based unlock
/// path (§6 — the only unlock path until Phase E device envelopes exist).
/// `kind:"rk"` is Phase D scope and answered UNKNOWN_OP.
pub fn begin_recovery_unlock(
    core: &Arc<Mutex<VaultCore>>,
    frame: &Value,
    deps: &Deps,
) -> OpOutcome {
    match frame.get("kind").and_then(Value::as_str) {
        Some("mp") => {}
        Some(_) => return OpOutcome::err(ErrorCode::UnknownOp),
        None => return OpOutcome::err(ErrorCode::InvalidInput),
    }
    let (header, vault_dir) = {
        let mut c = lock_core(core);
        if c.state != VaultState::Locked {
            return OpOutcome::err(ErrorCode::BadState);
        }
        if let Some(code) = c.header_error {
            // Boot-time header parse failure surfaces here (§3.6).
            c.enter_error(&deps.events);
            return OpOutcome::err(code);
        }
        let Some(header) = c.header.clone() else {
            return OpOutcome::err(ErrorCode::DbCorrupt);
        };
        c.state = VaultState::Unlocking;
        (header, c.vault_dir.clone())
    };
    deps.events.emit(ev_state(VaultState::Unlocking));
    emit_panel(deps, true, PanelRequest::MpEntry);
    let outcome = deps.panel.run(PanelRequest::MpEntry, PANEL_TIMEOUT);
    emit_panel(deps, false, PanelRequest::MpEntry);
    let PanelOutcome::Submitted(mp) = outcome else {
        // Cancelled or timed out: back to LOCKED unless a lock preempted
        // us while the panel was up.
        let mut c = lock_core(core);
        if c.state == VaultState::Unlocking {
            c.state = VaultState::Locked;
            deps.events.emit(ev_state(VaultState::Locked));
        }
        return OpOutcome::err(ErrorCode::PanelCancelled);
    };
    finish_mp_unlock(core, &header, &vault_dir, &mp, deps)
}

fn finish_mp_unlock(
    core: &Arc<Mutex<VaultCore>>,
    header: &Header,
    vault_dir: &std::path::Path,
    mp: &[u8],
    deps: &Deps,
) -> OpOutcome {
    let wrap_bytes = match std::fs::read(vault_dir.join(PASSWORD_WRAP_NAME)) {
        Ok(b) => b,
        Err(_) => return unlock_failed_nonfatal(core, ErrorCode::WrapCorrupt, deps),
    };
    let wrap_file: PasswordWrapFile = match serde_json::from_slice(&wrap_bytes) {
        Ok(f) => f,
        Err(_) => return unlock_failed_nonfatal(core, ErrorCode::WrapCorrupt, deps),
    };
    // The wrap file carries its own copy of the kdf block (§2.3); derive
    // from that copy so the unwrap is self-contained even if a crash ever
    // desynchronized header.json from the wrap.
    let (salt, params) = match wrap_kdf(&wrap_file) {
        Ok(v) => v,
        Err(e) => return unlock_failed_nonfatal(core, e, deps),
    };
    let Ok(pk) = kdf::derive_pk(mp, &salt, params) else {
        return unlock_failed_fatal(core, ErrorCode::Internal, deps);
    };
    // `mp` borrows the caller's SecretVec, which zeroizes on drop there.
    match wrap::open_wrap_mp(&wrap_file, &pk, &header.vault_id.0) {
        Err(crate::crypto::CryptoError::IntegrityFailure) => {
            // §15: wrong MP → backoff 500 ms ×2^attempts (cap 30 s), no
            // oracle beyond the attempts counter itself.
            let delay = {
                let mut c = lock_core(core);
                c.state = VaultState::Locked;
                deps.events.emit(ev_state(VaultState::Locked));
                c.record_failed_attempt()
            };
            std::thread::sleep(delay);
            OpOutcome::err(ErrorCode::WrongCredential)
        }
        Err(_) => unlock_failed_nonfatal(core, ErrorCode::WrapCorrupt, deps),
        Ok(payload) => install_unlock(core, header, vault_dir, payload, deps),
    }
}

/// Wrong-credential class failures stay LOCKED and retryable.
fn unlock_failed_nonfatal(
    core: &Arc<Mutex<VaultCore>>,
    code: ErrorCode,
    deps: &Deps,
) -> OpOutcome {
    let mut c = lock_core(core);
    c.state = VaultState::Locked;
    deps.events.emit(ev_state(VaultState::Locked));
    OpOutcome::err(code)
}

/// Vault-data failures enter ERROR (§3.6: the helper never deletes the
/// file; the user gets the restore path once Phase D lands).
fn unlock_failed_fatal(core: &Arc<Mutex<VaultCore>>, code: ErrorCode, deps: &Deps) -> OpOutcome {
    let mut c = lock_core(core);
    c.enter_error(&deps.events);
    OpOutcome::err(code)
}

fn install_unlock(
    core: &Arc<Mutex<VaultCore>>,
    header: &Header,
    vault_dir: &std::path::Path,
    payload: RecoveryWrapPayload,
    deps: &Deps,
) -> OpOutcome {
    if payload.vk_generation != header.vk_generation {
        // Wrap predates the header's generation — treated as wrap damage;
        // rotation rewrites both atomically (§2.10).
        return unlock_failed_nonfatal(core, ErrorCode::WrapCorrupt, deps);
    }
    let store = match VaultStore::open(vault_dir) {
        Ok(s) => s,
        Err(e) => return unlock_failed_fatal(core, e, deps),
    };
    // §2.8 rollback evidence: a directory older than the last generation
    // this helper has seen is refused.
    match keychain::read_seen_generation() {
        Ok(Some(seen)) if seen > header.manifest_generation => {
            return unlock_failed_fatal(core, ErrorCode::ManifestRollback, deps);
        }
        Err(_) => return unlock_failed_fatal(core, ErrorCode::Internal, deps),
        _ => {}
    }
    let mut c = lock_core(core);
    if c.state != VaultState::Unlocking {
        // A lock preempted while we unwrapped; VK drops here (zeroized).
        return OpOutcome::err(ErrorCode::BadState);
    }
    let generation = header.manifest_generation;
    c.vk = Some(payload.vk.mlock_best_effort());
    c.store = Some(store);
    c.header = Some(header.clone());
    c.failed_attempts = 0;
    c.note_authorization();
    c.state = VaultState::Unlocked;
    let _ = keychain::write_seen_generation(generation);
    deps.events.emit(ev_state(VaultState::Unlocked));
    OpOutcome::ok(json!({"state": "unlocked"}))
}

/// The wrap's own kdf block (§2.5 JSON shape), strictly parsed.
pub(super) fn wrap_kdf(file: &PasswordWrapFile) -> Result<([u8; 16], Argon2Params), ErrorCode> {
    if file.v != 1 || file.kind != "mp" || file.kdf_version != kdf::KDF_VERSION_V1 {
        // §15: unknown/future wrap kdf_version fails closed.
        return Err(ErrorCode::FormatTooNew);
    }
    let salt: [u8; 16] =
        crate::crypto::hex::decode_array(&file.argon2id.salt).ok_or(ErrorCode::WrapCorrupt)?;
    let params = Argon2Params {
        m: file.argon2id.m,
        t: file.argon2id.t,
        p: file.argon2id.p,
    };
    if params.m == 0 || params.t == 0 || params.p == 0 {
        return Err(ErrorCode::WrapCorrupt);
    }
    Ok((salt, params))
}
