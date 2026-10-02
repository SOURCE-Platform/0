//! Op dispatch for the `app` client class (§1.5). `get_state`, `lock` and
//! `hello` are answered by the server layer; the nm-host class is limited
//! to its own ops before anything reaches here (`ipc::conn`).

use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::*;
use crate::errors::ErrorCode;

/// Dispatch one post-hello op (§1.5 Phase C subset). `get_state`, `lock`,
/// and `hello` are handled by the server layer (they need no vault deps).
pub fn dispatch(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let op = frame.get("op").and_then(Value::as_str).unwrap_or("");
    if let Some(ev) = lock_core(core).expire_sessions() {
        deps.events.emit(ev);
    }
    if AUTHORING.contains(&op) && lock_core(core).behind {
        return OpOutcome::err(ErrorCode::VaultBehind);
    }
    // §22.4: a key no SE commitment vouches for serves reads only, for the
    // whole unlocked session (an allowlist: anything else is refused).
    if lock_core(core).unverified_key && !vk_commit::UNVERIFIED_OPS.contains(&op) {
        return OpOutcome::err(ErrorCode::DeviceNotAuthorized);
    }
    let out = route(core, frame, deps, op);
    {
        let mut c = lock_core(core);
        if c.vk_commit_pending {
            vk_commit::commit_resident(&mut c);
        }
    }
    if out.response["ok"] == Value::Bool(true) && FLOOR_OPS.contains(&op) {
        // Catch-up only on verified outcomes: "nothing newer than what you
        // accepted", or a completed apply (review VER-O12).
        let r = &out.response;
        let verified = r["up_to_date"] == true || r.get("admitted").is_some() || r["committed"] == true;
        tend_floor(core, verified);
        if verified && op != "backup_commit_result" {
            lock_core(core).provider_checked = Some(super::peer_serve::Checked::now());
        }
    }
    out
}

/// Ops after which the accepted provider state may have moved.
const FLOOR_OPS: &[&str] = &[
    "backup_state_offer", "backup_apply", "backup_commit_result",
    // Every local authority change is recorded at once (re-review SEC-B2).
    "change_master_password", "rotate_recovery_key", "revoke_device", "enroll_ack", "setup_retry_handle",
];

/// §22.14: every op that authors a revision or changes authority.
const AUTHORING: &[&str] = &[
    "add_item", "update_item", "delete_item", "restore_revision", "resolve_conflict", "backup_prepare", "change_master_password",
    "rotate_recovery_key", "setup_retry_handle", "revoke_device", "begin_enrollment", "enroll_hello", "enroll_confirm", "enroll_ack",
];

/// §22.14: while behind, a verified provider exchange may catch the store
/// up (never by lowering the floor); otherwise the floor follows the
/// accepted provider state.
fn tend_floor(core: &Arc<Mutex<VaultCore>>, verified: bool) {
    let mut c = lock_core(core);
    let behind = c.behind;
    let Some(store) = c.store.as_mut() else {
        return;
    };
    if !behind {
        let _ = floor::raise(store);
    } else if verified && floor::catch_up(store).unwrap_or(false) {
        c.header = c.store.as_ref().map(|s| s.header.clone());
        c.behind = false;
    }
}

fn route(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps, op: &str) -> OpOutcome {
    match op {
        "setup_vault" => setup::setup_vault(core, frame, deps),
        "unlock" => device_unlock::unlock(core, deps),
        "begin_recovery_unlock" => setup::begin_recovery_unlock(core, frame, deps),
        "change_master_password" if frame.get("mode").and_then(Value::as_str) == Some("reset") => {
            rk_ops::reset_master_password(core, deps)
        }
        "change_master_password" => change_mp::change_master_password(core, deps),
        "rotate_recovery_key" => rk_ops::rotate_recovery_key(core, frame, deps),
        "setup_retry_handle" => retry_handle::setup_retry_handle(core, frame, deps),
        "list_items" => items::list_items(core),
        "add_item" => items::add_item(core, frame, deps),
        "update_item" => items::update_item(core, frame, deps),
        "delete_item" => items::delete_item(core, frame, deps),
        "reveal" => gate::reveal(core, frame, deps),
        "peer_serve" => peer_serve::peer_serve(core, frame),
        "peer_serve_begin" => peer_serve::peer_serve_begin(core, frame),
        "list_history" => history::list_history(core, frame),
        "list_deleted" => history::list_deleted(core),
        "restore_revision" => history::restore_revision(core, frame, deps),
        "resolve_conflict" => resolve::resolve_conflict(core, frame, deps),
        "begin_enrollment" => enroll_ops::begin_enrollment(core, frame),
        "enroll_hello" => enroll_ops::enroll_hello(core, frame),
        "enroll_confirm" => enroll_ops::enroll_confirm(core, deps),
        "enroll_ack" => enroll_commit::enroll_ack(core, frame),
        "cancel_enrollment" => enroll_ops::cancel_enrollment(core),
        "list_devices" => devices::list_devices(core),
        "registry_status" => registry_status::registry_status(core),
        "revoke_device" => devices::revoke_device(core, frame, deps),
        "set_auto_lock_minutes" => prefs::set_auto_lock_minutes(core, frame),
        // v0.4 provider work (§1.3, §1.5, §11).
        "backup_prepare" => backup_ops::backup_prepare(core, deps),
        "backup_blob_list" => backup_ops::backup_blob_list(core, frame),
        "backup_transition_body" => backup_ops::backup_transition_body(core, frame),
        "backup_commit_result" => backup_ops::backup_commit_result(core, frame, deps),
        "backup_state_offer" => sync_ops::backup_state_offer(core, frame, deps),
        "backup_apply" => sync_ops::backup_apply(core, frame, deps),
        "stream_read" => provider_ops::stream_read(core, frame),
        "stream_begin" => provider_ops::stream_begin(core, frame),
        "stream_write" | "stream_end" | "stream_cancel" => provider_ops::stream_io(core, frame, op),
        "sign_provider_request" => provider_ops::sign_provider_request(core, frame),
        "session_close" => provider_ops::session_close(core, frame, deps),
        "quarantine_status" => provider_ops::quarantine_status(core),
        "remote_update_status" => remote_status::remote_update_status(core),
        "recovery_begin" => recovery_flow::recovery_begin(core, frame, deps),
        "recovery_preview" => recovery_flow::recovery_preview(core),
        "recovery_complete" => recovery_flow::recovery_complete(core, deps),
        _ => OpOutcome::err(ErrorCode::UnknownOp),
    }
}

