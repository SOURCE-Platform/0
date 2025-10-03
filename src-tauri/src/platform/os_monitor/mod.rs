// OS activity monitoring - tracks application lifecycle and focus

use crate::models::activity::{AppEvent, AppInfo};
use tokio::sync::mpsc;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

/// Platform-agnostic OS monitor trait
pub trait OSMonitor: Send + Sync {
    /// Start monitoring OS activity
    fn start_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error>>;

    /// Stop monitoring OS activity
    fn stop_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error>>;

    /// Get list of currently running applications
    fn get_running_apps(&self) -> Result<Vec<AppInfo>, Box<dyn std::error::Error>>;

    /// Get the frontmost (focused) application
    fn get_frontmost_app(&self) -> Result<Option<AppInfo>, Box<dyn std::error::Error>>;

    /// Check if monitoring is currently active
    fn is_monitoring(&self) -> bool;
}

/// Create a platform-specific OS monitor
pub fn create_os_monitor() -> Result<(Box<dyn OSMonitor>, mpsc::UnboundedReceiver<AppEvent>), Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    {
        macos::MacOSMonitor::new()
    }

    #[cfg(target_os = "windows")]
    {
        windows::WindowsMonitor::new()
    }

    #[cfg(target_os = "linux")]
    {
        linux::LinuxMonitor::new()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("OS monitoring not supported on this platform".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_os_monitor() {
        let result = create_os_monitor();

        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        {
            assert!(result.is_ok(), "Should create monitor on supported platform");
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            assert!(result.is_err(), "Should fail on unsupported platform");
        }
    }
}
