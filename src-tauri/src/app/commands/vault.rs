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

/// Yield activation to the helper and **wait for it to happen** on the
/// main thread. SOURCE is the active app when the user clicks; without
/// this the helper's panel — or its Touch ID sheet — opens unfocused and
/// can sit behind our own window (UI-02).
async fn yield_activation(app: tauri::AppHandle) {
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
}

/// An op that presents the helper's secure panel.
async fn call_with_panel(app: tauri::AppHandle, frame: Value) -> Result<Value, String> {
    yield_activation(app).await;
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
    // §2.8: the ordinary path is this device's own envelope — presence,
    // then the Secure Enclave hands back the vault key. The master
    // password is the fallback, for a device with no usable envelope
    // (no SE key, not enrolled, or an envelope left behind by a
    // rotation), not the default.
    yield_activation(app.clone()).await;
    match call(json!({"op": "unlock"})).await {
        Ok(resp) if resp["ok"] == Value::Bool(true) => Ok(resp),
        Ok(resp) if uses_master_password_instead(&resp) => {
            call_with_panel(app, json!({"op": "begin_recovery_unlock", "kind": "mp"})).await
        }
        // A refused presence check is the user saying no. Falling through
        // to a password prompt would turn "cancel" into "try harder".
        other => other,
    }
}

/// Envelope unlock is unavailable on this device — fall back to the
/// master password. Anything else (a denied presence check, damaged
/// vault data) is reported as itself.
fn uses_master_password_instead(resp: &Value) -> bool {
    matches!(
        resp["error"].as_str(),
        Some("DEVICE_NOT_AUTHORIZED") | Some("WRAP_CORRUPT") | Some("NOT_FOUND")
    )
}

/// Explicitly unlock with the master password, whatever this device's
/// envelope says (§1.5 `begin_recovery_unlock`).
#[tauri::command]
pub async fn vault_unlock_with_master_password(app: tauri::AppHandle) -> Result<Value, String> {
    call_with_panel(app, json!({"op": "begin_recovery_unlock", "kind": "mp"})).await
}

/// Re-wrap the vault key under a new master password (UNLOCKED only). The
/// helper's panel collects current + new + confirm; nothing secret
/// crosses this process.
#[tauri::command]
pub async fn vault_change_master_password(app: tauri::AppHandle) -> Result<Value, String> {
    call_with_panel(app, json!({"op": "change_master_password"})).await
}

/// Unlock with the 24-word Recovery Key, typed into the helper's own
/// panel (§1.5 begin_recovery_unlock kind "rk"). The words never cross.
#[tauri::command]
pub async fn vault_unlock_with_recovery_key(app: tauri::AppHandle) -> Result<Value, String> {
    call_with_panel(app, json!({"op": "begin_recovery_unlock", "kind": "rk"})).await
}

/// §12 scenarios 6/7: new Recovery Key + vault-key rotation. The helper
/// shows (and can print) the new words in its own capture-excluded window.
#[tauri::command]
pub async fn vault_rotate_recovery_key(app: tauri::AppHandle) -> Result<Value, String> {
    call_with_panel(app, json!({"op": "rotate_recovery_key"})).await
}

/// §12 scenario 5: set a new master password on an unlocked vault when
/// the old one is forgotten (e.g. after a Recovery Key unlock).
#[tauri::command]
pub async fn vault_reset_master_password(app: tauri::AppHandle) -> Result<Value, String> {
    call_with_panel(app, json!({"op": "change_master_password", "mode": "reset"})).await
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

// --- Phase E: devices and enrollment (§5, §11.4) ---------------------------

/// Start an enrollment: ephemeral TLS server, helper-minted single-use
/// secret, QR for the screen (§5.1).
#[tauri::command]
pub async fn vault_begin_enrollment() -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        crate::core::vault_enroll::begin("SOURCE").await
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("vault is not available on this platform".to_string())
    }
}

/// Poll while the QR is on screen: reports the SAS once the phone's
/// hello has been authenticated, and whether the ACK has landed.
#[tauri::command]
pub async fn vault_enrollment_status() -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        Ok(crate::core::vault_enroll::status())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(json!({"ok": true, "active": false}))
    }
}

/// The user compared the SAS on both screens and confirmed. The helper
/// runs its own presence check before it builds anything (§5.1).
#[tauri::command]
pub async fn vault_confirm_enrollment(app: tauri::AppHandle) -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        // The helper raises a Touch ID sheet before it signs anything, so
        // this waits for activation to reach it exactly like the panel
        // ops do. Firing and forgetting leaves the sheet behind our
        // window and the enrollment looks hung (observed on hardware).
        yield_activation(app).await;
        let result = crate::core::vault_enroll::confirm().await;
        if let Err(ref e) = result {
            eprintln!("vault: enrollment confirm failed: {e}");
        }
        result
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err("vault is not available on this platform".to_string())
    }
}

#[tauri::command]
pub async fn vault_cancel_enrollment() -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        crate::core::vault_enroll::cancel().await;
    }
    Ok(json!({"ok": true}))
}

#[tauri::command]
pub async fn vault_list_devices() -> Result<Value, String> {
    call(json!({"op": "list_devices"})).await
}

/// Revoke a device: registry entry + mandatory VK rotation. The helper
/// asks for presence, the master password, and shows a new Recovery Key
/// before anything is committed (§11.4, §12 scenario 8).
#[tauri::command]
pub async fn vault_revoke_device(
    app: tauri::AppHandle,
    device_id: String,
) -> Result<Value, String> {
    call_with_panel(app, json!({"op": "revoke_device", "device_id": device_id})).await
}
