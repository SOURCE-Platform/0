//! Chrome's native-messaging registration for the vault broker (spec
//! §9.2; Phase H). Written by the app once it has reached the helper:
//! `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/
//! com.racker.zero.vault.json`, naming the signed `source-vault-nm-host`
//! and only the pinned extension.
//!
//! - The broker binary must pass its designated requirement
//!   (`com.racker.zero.nm-host`, our team) before it is registered.
//! - Debug builds register the development broker next to the dev helper
//!   bundle with the unpacked extension's pinned id; release builds
//!   register `SOURCE.app/Contents/Library/vault-nm-host/…` with the
//!   store id, and write nothing while that id is unset.
//! - Chrome only: §9.2 layer 3 has a reviewed requirement row for Chrome
//!   alone; Chromium, Edge, Arc and Brave stay unsupported until theirs
//!   are reviewed.
//! - Written only when Chrome's profile folder exists, and only when the
//!   content changed.

use std::path::PathBuf;

use serde_json::json;

const HOST_NAME: &str = "com.racker.zero.vault";

#[cfg(debug_assertions)]
fn extension_id() -> Option<&'static str> {
    Some(include_str!("../../../browser-extension/DEV_EXTENSION_ID").trim())
}

#[cfg(not(debug_assertions))]
fn extension_id() -> Option<&'static str> {
    Some(vault_helper::nm_host::PRODUCTION_ID).filter(|id| !id.is_empty())
}

fn broker_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    let path = if cfg!(debug_assertions) {
        // target/<profile>/<app> → target/helper-bundle/debug/vault-nm-host/
        exe_dir.parent()?.join("helper-bundle/debug/vault-nm-host/source-vault-nm-host")
    } else {
        // SOURCE.app/Contents/MacOS/<app> → Contents/Library/vault-nm-host/
        exe_dir.parent()?.join("Library/vault-nm-host/source-vault-nm-host")
    };
    path.is_file().then_some(path)
}

/// The manifest text for this broker and extension.
pub fn manifest(path: &std::path::Path, extension_id: &str) -> String {
    let m = json!({
        "name": HOST_NAME,
        "description": "Source Vault native messaging host",
        "path": path,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{extension_id}/")],
    });
    serde_json::to_string_pretty(&m).unwrap_or_default()
}

/// Register the broker with Chrome, or say why not. Never fails the app.
pub fn install() -> Result<bool, String> {
    let Some(id) = extension_id() else { return Err("no extension id for this build".into()) };
    let Some(path) = broker_path() else { return Err("nm-host not built (scripts/build-helper.sh)".into()) };
    vault_helper::ipc::peer_auth::verify_nm_host_binary(&path).map_err(|e| format!("nm-host signature: {e}"))?;
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("no HOME")?;
    let chrome = home.join("Library/Application Support/Google/Chrome");
    if !chrome.is_dir() {
        return Ok(false);
    }
    let dir = chrome.join("NativeMessagingHosts");
    let file = dir.join(format!("{HOST_NAME}.json"));
    let text = manifest(&path, id);
    if std::fs::read_to_string(&file).ok().as_deref() == Some(text.as_str()) {
        return Ok(false);
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, &text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_names_one_extension_and_the_broker() {
        let text = manifest(std::path::Path::new("/x/source-vault-nm-host"), "abcdefghijklmnopabcdefghijklmnop");
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["name"], HOST_NAME);
        assert_eq!(v["type"], "stdio");
        assert_eq!(v["path"], "/x/source-vault-nm-host");
        assert_eq!(v["allowed_origins"], json!(["chrome-extension://abcdefghijklmnopabcdefghijklmnop/"]));
    }
}
