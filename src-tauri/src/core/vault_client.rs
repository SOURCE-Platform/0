//! Main-app side of the vault-helper boundary (spec §1.4, §1.6, §14).
//!
//! The manager lazily launches the signed helper (§1.6 startup sequence,
//! deferred to first vault use so unsigned dev builds keep the rest of the
//! app functional), holds the single `app`-class connection, pumps helper
//! events to the frontend, and wires §14:
//!
//! - `secure_panel_visible` events bump the sensitive-surface counter for
//!   the exact panel lifetime and register the panel's window title in
//!   the capture-exclusion registry (§14.2);
//! - `capture_check` reverse queries are answered from the live capture
//!   registry — `screen_capture_suppressed()` (§14.4); any doubt answers
//!   "not suppressed", which makes the helper refuse the release.
//!
//! No secret material is stored here. Reveal responses pass through to
//! the caller and are never logged.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::{AppHandle, Emitter};
use vault_helper::ipc::client::{ClientError, VaultClient};
use vault_helper::ops::ClientClass;

use crate::core::capture_exclusions;

/// Tauri event channel carrying helper events to the frontend.
pub const VAULT_EVENT_CHANNEL: &str = "vault:event";

/// How long to wait for the helper's socket to appear after spawning.
const LAUNCH_WAIT: Duration = Duration::from_secs(5);

pub struct VaultManager {
    client: Mutex<Option<Arc<VaultClient>>>,
    child: Mutex<Option<Child>>,
    app: OnceLock<AppHandle>,
}

impl Default for VaultManager {
    fn default() -> Self {
        VaultManager {
            client: Mutex::new(None),
            child: Mutex::new(None),
            app: OnceLock::new(),
        }
    }
}

static MANAGER: OnceLock<VaultManager> = OnceLock::new();

pub fn manager() -> &'static VaultManager {
    MANAGER.get_or_init(VaultManager::default)
}

/// Called once from app setup. Stores the app handle for event emission;
/// does NOT launch the helper (that happens on first vault op, so dev
/// builds without a signed helper keep the app fully usable).
pub fn init(app: &AppHandle) {
    let _ = manager().app.set(app.clone());
}

/// §1.6 shutdown: send `lock` (helper zeroizes VK) and let the process
/// exit by dropping the connection (idle/grace handles the rest).
pub fn shutdown() {
    let client = manager()
        .client
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(client) = client {
        let _ = client.lock();
    }
}

/// Send one op frame; connects (launching the helper if needed) first.
/// A dead connection is dropped so the next call re-connects.
pub fn request(frame: Value) -> Result<Value, String> {
    let client = ensure_connected()?;
    match client.request(frame) {
        Ok(resp) => Ok(resp),
        Err(ClientError::Server(code)) => Err(code),
        Err(e) => {
            drop_client();
            Err(format!("HELPER_UNAVAILABLE ({e})"))
        }
    }
}

fn drop_client() {
    *manager().client.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn ensure_connected() -> Result<Arc<VaultClient>, String> {
    let mgr = manager();
    let mut slot = mgr.client.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(client) = slot.as_ref() {
        return Ok(Arc::clone(client));
    }
    let socket = vault_helper::socket_path();
    let client = try_connect(&socket)
        .or_else(|_| launch_and_connect(mgr, &socket))
        .map_err(|e| format!("HELPER_UNAVAILABLE ({e})"))?;
    let client = Arc::new(client);
    wire_after_connect(&client);
    *slot = Some(Arc::clone(&client));
    Ok(client)
}

fn try_connect(socket: &std::path::Path) -> Result<VaultClient, ClientError> {
    VaultClient::connect(socket, ClientClass::App)
}

fn launch_and_connect(
    mgr: &VaultManager,
    socket: &std::path::Path,
) -> Result<VaultClient, ClientError> {
    let binary = helper_binary().ok_or_else(|| {
        ClientError::Protocol(
            "helper bundle not found (build it with scripts/build-helper.sh)".to_string()
        )
    })?;
    // §1.4 item 1: static bundle check before the process is launched.
    vault_helper::ipc::peer_auth::verify_helper_bundle(&bundle_root(&binary))
        .map_err(ClientError::Auth)?;
    eprintln!("vault: launching helper at {}", binary.display());
    let child = Command::new(&binary)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(ClientError::Io)?;
    *mgr.child.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
    let deadline = Instant::now() + LAUNCH_WAIT;
    loop {
        if socket.exists() {
            if let Ok(client) = try_connect(socket) {
                return Ok(client);
            }
        }
        if Instant::now() >= deadline {
            return Err(ClientError::Protocol(
                "helper did not become ready in time".to_string(),
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Resolve the helper executable. Order: debug env override; production
/// nested-bundle layout (Contents/MacOS sibling); dev build produced by
/// `scripts/build-helper.sh debug` (target/helper-bundle/debug); bare
/// sibling binary last.
fn helper_binary() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    if let Ok(path) = std::env::var("OV0_VAULT_HELPER_PATH") {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    // The signed dev bundle must be tried before the bare sibling: in a
    // dev build `target/<profile>/source-vault-helper` is cargo's unsigned
    // output, which the pre-launch signature check rightly rejects.
    let candidates = [
        exe_dir
            .join("SourceVaultHelper.app")
            .join("Contents")
            .join("MacOS")
            .join("source-vault-helper"),
        // Dev layout: target/<profile>/<app> → target/helper-bundle/<profile>/...
        exe_dir
            .parent()
            .map(|target| target.join("helper-bundle"))
            .map(|d| {
                d.join(if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                })
            })
            .unwrap_or_default()
            .join("SourceVaultHelper.app")
            .join("Contents")
            .join("MacOS")
            .join("source-vault-helper"),
        exe_dir.join("source-vault-helper"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

/// The bundle root for the pre-launch static check: walk up out of
/// `Contents/MacOS/<bin>` when the binary lives inside a bundle.
fn bundle_root(binary: &std::path::Path) -> PathBuf {
    binary
        .ancestors()
        .nth(2)
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
        .unwrap_or(binary)
        .to_path_buf()
}

/// After connect: install the §14.4 capture-check handler and spawn the
/// event pump (helper events → capture exclusions + frontend).
fn wire_after_connect(client: &Arc<VaultClient>) {
    client.set_capture_handler(Arc::new(|_surface| {
        capture_exclusions::screen_capture_suppressed()
    }));
    let client = Arc::clone(client);
    std::thread::spawn(move || loop {
        let Some(event) = client.recv_event() else {
            // Helper connection gone; drop the cached client so the next
            // op reconnects (§1.6: helper crash kills VK; state is LOCKED
            // after restart).
            drop_client();
            emit_frontend(&serde_json::json!({"event": "state", "state": "helper-gone"}));
            return;
        };
        apply_capture_wiring(&event);
        emit_frontend(&event);
    });
}

/// §14.2: while a helper panel is visible, bump the sensitive-surface
/// counter (exact panel lifetime) and register its exact window title.
fn apply_capture_wiring(event: &Value) {
    if event.get("event").and_then(Value::as_str) != Some("secure_panel_visible") {
        return;
    }
    let visible = event.get("visible").and_then(Value::as_bool).unwrap_or(false);
    if visible {
        if let Some(title) = event.get("title").and_then(Value::as_str) {
            capture_exclusions::register_excluded_window_title(title);
        }
        capture_exclusions::sensitive_surface_shown();
    } else {
        capture_exclusions::sensitive_surface_hidden();
    }
}

fn emit_frontend(event: &Value) {
    if let Some(app) = manager().app.get() {
        let _ = app.emit(VAULT_EVENT_CHANNEL, event.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// §14.2 wiring, UI-01/UI-03 main-app side: a helper panel becoming
    /// visible suppresses frame capture (hence OCR, which only sees
    /// captured frames) and registers its title; the matching hidden event
    /// releases that hold. The counter is process-global and shared with
    /// other tests, so only the held state is asserted absolutely.
    #[test]
    fn secure_panel_events_drive_capture_suppression() {
        let title = "Source Vault — Unlock (wiring test)";
        apply_capture_wiring(&json!({
            "event": "secure_panel_visible", "visible": true, "title": title,
        }));
        assert!(capture_exclusions::screen_capture_suppressed());
        assert!(capture_exclusions::is_excluded_window_title(title));
        apply_capture_wiring(&json!({"event": "secure_panel_visible", "visible": false}));
        // Unrelated events never touch the registry.
        apply_capture_wiring(&json!({"event": "state", "state": "unlocked"}));
        capture_exclusions::unregister_excluded_window_title(title);
    }
}
