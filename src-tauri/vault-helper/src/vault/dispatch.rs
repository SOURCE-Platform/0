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
    match op {
        "setup_vault" => setup::setup_vault(core, frame, deps),
        "unlock" => device_unlock::unlock(core, deps),
        "begin_recovery_unlock" => setup::begin_recovery_unlock(core, frame, deps),
        "change_master_password" if frame.get("mode").and_then(Value::as_str) == Some("reset") => {
            rk_ops::reset_master_password(core, deps)
        }
        "change_master_password" => change_mp::change_master_password(core, deps),
        "rotate_recovery_key" => rk_ops::rotate_recovery_key(core, deps),
        "list_items" => items::list_items(core),
        "add_item" => items::add_item(core, frame, deps),
        "update_item" => items::update_item(core, frame, deps),
        "delete_item" => items::delete_item(core, frame, deps),
        "reveal" => gate::reveal(core, frame, deps),
        "resolve_conflict" => resolve::resolve_conflict(core, frame, deps),
        "begin_enrollment" => enroll_ops::begin_enrollment(core, frame),
        "enroll_hello" => enroll_ops::enroll_hello(core, frame),
        "enroll_confirm" => enroll_ops::enroll_confirm(core, deps),
        "enroll_ack" => enroll_commit::enroll_ack(core, frame),
        "cancel_enrollment" => enroll_ops::cancel_enrollment(core),
        "list_devices" => devices::list_devices(core),
        "registry_status" => registry_status::registry_status(core),
        "revoke_device" => devices::revoke_device(core, frame, deps),
        "set_auto_lock_minutes" => super::set_auto_lock_minutes(core, frame),
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
        "session_close" => provider_ops::session_close(core, frame),
        "quarantine_status" => provider_ops::quarantine_status(core),
        "recovery_begin" => recovery_flow::recovery_begin(core, frame, deps),
        "recovery_preview" => recovery_flow::recovery_preview(core),
        "recovery_complete" => recovery_flow::recovery_complete(core, deps),
        _ => OpOutcome::err(ErrorCode::UnknownOp),
    }
}

