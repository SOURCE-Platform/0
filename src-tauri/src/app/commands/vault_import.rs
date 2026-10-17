//! Dashlane import commands (Phase I, spec §10.4–§10.6).
//!
//! The file picker runs here, not in the web view, so page code never
//! chooses which path the helper opens; the helper re-checks the path and
//! opens the file itself. The picked path is kept in this process only so
//! the user's later "Move the export to the Trash" click acts on the same
//! file — it is never sent back to the page.

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{json, Value};
use tauri_plugin_dialog::DialogExt;

use super::vault::call_with_panel;

static LAST_EXPORT: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Pick an export and import it. `{"cancelled": true}` when the user
/// closes the picker; otherwise the helper's answer with its report.
#[tauri::command]
pub async fn vault_import_dashlane(app: tauri::AppHandle) -> Result<Value, String> {
    let picker = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        picker
            .dialog()
            .file()
            .set_title("Choose your Dashlane export")
            .add_filter("Dashlane export", &["csv", "zip"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("picker failed: {e}"))?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(json!({"ok": true, "cancelled": true}));
    };
    let resp = call_with_panel(app, json!({"op": "import_dashlane", "path": path.to_string_lossy()})).await?;
    // §10.6: the export may be moved to the Trash only when everything in
    // it came in (final review SPEC-I5); otherwise it may be the only copy.
    let complete = resp["ok"] == true && resp["report"]["keep_export"] == false;
    *LAST_EXPORT.lock().unwrap_or_else(|e| e.into_inner()) = complete.then_some(path);
    Ok(resp)
}

/// Move the export just imported to the Trash, on the user's click
/// (§10.6). Once only; the path is forgotten either way.
#[tauri::command]
pub async fn vault_trash_export() -> Result<Value, String> {
    let Some(path) = LAST_EXPORT.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return Err("NO_EXPORT".to_string());
    };
    let moved = tauri::async_runtime::spawn_blocking(move || crate::platform::trash::move_to_trash(&path))
        .await
        .map_err(|e| format!("trash failed: {e}"))?;
    Ok(json!({"ok": moved}))
}
