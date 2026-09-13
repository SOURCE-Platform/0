use crate::platform::capture::{request_screen_capture_permission, screen_capture_permission_granted};

/// Registers SOURCE with macOS (so it appears in the list) and opens the
/// Screen & System Audio Recording page where the user can switch it on.
#[tauri::command]
pub fn open_screen_capture_settings() -> Result<(), String> {
    if !screen_capture_permission_granted() {
        request_screen_capture_permission();
    }
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        .spawn()
        .map_err(|e| format!("Failed to open System Settings: {e}"))?;
    Ok(())
}
