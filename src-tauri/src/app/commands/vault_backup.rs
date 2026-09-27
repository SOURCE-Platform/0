//! Tauri commands for the vault backup (spec v0.4 §11): status, "back up
//! now", and total-loss recovery on a machine without a vault. The
//! helper's own panel collects the MP/RK; this process only moves bytes
//! and never sees a secret.

use serde_json::{json, Value};

/// Current backup status (the worker also pushes it on `vault:backup`).
#[tauri::command]
pub async fn vault_backup_status() -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        let origin = crate::core::vault_backup::origin();
        Ok(json!({ "status": crate::core::vault_backup::worker::status(), "provider": origin }))
    }
    #[cfg(not(target_os = "macos"))]
    Err("vault is not available on this platform".to_string())
}

/// Run a backup cycle now (§11.3.2 "retry now").
#[tauri::command]
pub async fn vault_backup_now() -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    crate::core::vault_backup::worker::trigger(crate::core::vault_backup::worker::Trigger::Now);
    Ok(json!({ "ok": true }))
}

#[cfg(target_os = "macos")]
async fn flows_blocking<T: Send + 'static>(f: impl FnOnce(&vault_coordinator::flows::Flows<'_>) -> T + Send + 'static) -> Result<T, String> {
    use crate::core::vault_backup::{transport::ProviderHttp, AppHelper};
    tauri::async_runtime::spawn_blocking(move || {
        let transport = ProviderHttp;
        f(&vault_coordinator::flows::Flows { helper: &AppHelper, transport: &transport })
    })
    .await
    .map_err(|e| format!("vault task join failed: {e}"))
}

#[cfg(target_os = "macos")]
fn failure(f: vault_coordinator::Failure) -> String {
    match f {
        vault_coordinator::Failure::Helper(c) => c,
        vault_coordinator::Failure::Provider(_, c) if !c.is_empty() => c,
        vault_coordinator::Failure::Provider(s, _) => format!("PROVIDER_{s}"),
        vault_coordinator::Failure::Unreachable => "BACKUP_UNAVAILABLE".into(),
        vault_coordinator::Failure::Conflict => "BACKUP_CONFLICT".into(),
    }
}

/// Total-loss recovery, first half (§11.5): locate by handle, the helper's
/// panel collects the master password (`kind: "mp"`) or Recovery Key
/// (`"rk"`), then download and verify. Returns the FR-01 preview (date,
/// generation, item count) for the user to confirm before finishing.
#[tauri::command]
pub async fn vault_recovery_start(app: tauri::AppHandle, handle: String, kind: String) -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        let origin = crate::core::vault_backup::origin().ok_or("BACKUP_UNAVAILABLE")?;
        super::vault::yield_activation(app).await;
        flows_blocking(move |f| f.recovery_start(origin, &handle, &kind)).await?.map_err(failure)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, handle, kind);
        Err("vault is not available on this platform".to_string())
    }
}

/// Total-loss recovery, second half (§11.8): re-encrypt under a fresh key
/// (a new Recovery Key is shown in the helper's window), upload, finalize.
#[tauri::command]
pub async fn vault_recovery_finish(app: tauri::AppHandle) -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        super::vault::yield_activation(app).await;
        flows_blocking(|f| f.recovery_finish()).await?.map_err(failure)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err("vault is not available on this platform".to_string())
    }
}
