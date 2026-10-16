//! Joining a vault from an iPhone (F.2e, spec §22.13): the Vault tab of a
//! Mac with no vault shows a QR for the iPhone to scan; the helper's own
//! "Join a Vault" window shows the code and asks for the master password.

use serde_json::{json, Value};

/// Start: the QR (`v:3`), valid five minutes.
#[tauri::command]
pub async fn vault_join_begin() -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        crate::core::vault_enroll::join::begin("Mac").await
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("vault is not available on this platform".to_string())
    }
}

/// Polled while the QR is on screen: waiting, comparing, confirmed,
/// joined, failed.
#[tauri::command]
pub async fn vault_join_status(app: tauri::AppHandle) -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        let s = crate::core::vault_enroll::join::status();
        // The helper's panel needs the app in front once the iPhone said hello.
        if s["stage"] == "comparing" {
            super::vault::yield_activation(app).await;
        }
        Ok(s)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(json!({"ok": true, "active": false}))
    }
}

#[tauri::command]
pub async fn vault_join_cancel() -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        crate::core::vault_enroll::join::cancel().await;
    }
    Ok(json!({"ok": true}))
}
