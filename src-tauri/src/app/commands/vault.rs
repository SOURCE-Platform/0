//! Tauri commands for the credential vault surface (Phase C, macOS).
//!
//! Thin wrappers over `core::vault_client`: every command forwards one op
//! frame to the helper and returns its response. No vault state is cached
//! in this process; reveal responses pass straight through and are never
//! logged (§15). Blocking socket calls run on `spawn_blocking` so the UI
//! thread never stalls behind a panel/LA wait (up to §13.3's 120 s).

use serde_json::{json, Value};

#[cfg(target_os = "macos")]
use crate::core::vault_client;

/// One blocking op on a runtime worker thread.
#[cfg(target_os = "macos")]
async fn call(frame: Value) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || vault_client::request(frame))
        .await
        .map_err(|e| format!("vault task join failed: {e}"))?
}

#[cfg(not(target_os = "macos"))]
async fn call(_frame: Value) -> Result<Value, String> {
    Err("vault is not available on this platform".to_string())
}

/// An op that presents the helper's secure panel. SOURCE is the active app
/// when the user clicks, so it yields activation to the helper first
/// (macOS 14 cooperative activation); otherwise the panel opens unfocused
/// and its secure field never receives keystrokes until clicked (UI-02).
/// The yield completes on the main thread before the op is sent.
async fn call_with_panel(app: tauri::AppHandle, frame: Value) -> Result<Value, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let queued = app.run_on_main_thread(move || {
        crate::platform::activation::yield_activation_to(
            crate::platform::activation::VAULT_HELPER_BUNDLE_ID,
        );
        let _ = tx.send(());
    });
    if queued.is_ok() {
        let _ = tauri::async_runtime::spawn_blocking(move || {
            rx.recv_timeout(std::time::Duration::from_secs(2))
        })
        .await;
    }
    call(frame).await
}

/// Current state-machine state. Always answers (helper down →
/// `{"state": "helper-unavailable"}`) so the tab can render honestly.
#[tauri::command]
pub async fn vault_state() -> Result<Value, String> {
    match call(json!({"op": "get_state"})).await {
        Ok(resp) => Ok(resp),
        Err(e) if e.starts_with("HELPER_UNAVAILABLE") => {
            Ok(json!({"ok": true, "state": "helper-unavailable"}))
        }
        Err(e) => Err(e),
    }
}

/// First-device vault creation (UNINITIALIZED only). The helper's secure
/// panel collects the new master password; nothing secret crosses here.
#[tauri::command]
pub async fn vault_setup(app: tauri::AppHandle) -> Result<Value, String> {
    call_with_panel(app, json!({"op": "setup_vault"})).await
}

/// Unlock via the helper's secure panel (§1.5 begin_recovery_unlock,
/// kind "mp" — the LA/device-envelope `unlock` op is Phase E).
#[tauri::command]
pub async fn vault_unlock(app: tauri::AppHandle) -> Result<Value, String> {
    call_with_panel(app, json!({"op": "begin_recovery_unlock", "kind": "mp"})).await
}

/// Re-wrap the vault key under a new master password (UNLOCKED only). The
/// helper's panel collects current + new + confirm; nothing secret
/// crosses this process.
#[tauri::command]
pub async fn vault_change_master_password(app: tauri::AppHandle) -> Result<Value, String> {
    call_with_panel(app, json!({"op": "change_master_password"})).await
}

#[tauri::command]
pub async fn vault_lock() -> Result<Value, String> {
    call(json!({"op": "lock"})).await
}

/// Metadata list only — `[{ref, kind, title, username, hosts}]` (§1.5).
#[tauri::command]
pub async fn vault_list_items() -> Result<Value, String> {
    call(json!({"op": "list_items"})).await
}

/// Add one login record. The field values cross IPC exactly once,
/// main→helper (§1.5). Synthetic data only in Phase C.
#[tauri::command]
pub async fn vault_add_login(
    title: String,
    username: String,
    host: String,
    password: String,
) -> Result<Value, String> {
    call(json!({
        "op": "add_item",
        "kind": "login",
        "title": title,
        "username": username,
        "hosts": [host],
        "password": password,
    }))
    .await
}

/// Edit one whitelisted field (title/username/password/host). The helper
/// enforces the same whitelist; the double gate is deliberate.
#[tauri::command]
pub async fn vault_update_item(
    reference: String,
    field: String,
    value: String,
) -> Result<Value, String> {
    if !matches!(field.as_str(), "title" | "username" | "password" | "host") {
        return Err("INVALID_INPUT".to_string());
    }
    call(json!({"op": "update_item", "ref": reference, field: value})).await
}

/// Tombstone one record (§3.2 revision model).
#[tauri::command]
pub async fn vault_delete_item(reference: String) -> Result<Value, String> {
    call(json!({"op": "delete_item", "ref": reference})).await
}

/// One-shot reveal of one record's secret fields (§14.4: the helper
/// verifies capture suppression with this process first; refusal is
/// `CAPTURE_UNSAFE` and nothing is returned).
#[tauri::command]
pub async fn vault_reveal(reference: String) -> Result<Value, String> {
    call(json!({"op": "reveal", "ref": reference})).await
}

/// §1.6 auto-lock dial (5–60 minutes). Internal op documented as a
/// Phase C deviation in the verification report.
#[tauri::command]
pub async fn vault_set_auto_lock_minutes(minutes: u32) -> Result<Value, String> {
    call(json!({"op": "set_auto_lock_minutes", "minutes": minutes})).await
}
