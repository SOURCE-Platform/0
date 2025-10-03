// macOS application monitoring using NSWorkspace

use crate::models::activity::{AppEvent, AppInfo};
use crate::platform::os_monitor::OSMonitor;
use cocoa::base::{id, nil};
use objc::{class, msg_send, sel, sel_impl};
use std::ffi::CStr;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// macOS application monitor using NSWorkspace
pub struct MacOSMonitor {
    is_monitoring: Arc<Mutex<bool>>,
    event_sender: Arc<Mutex<mpsc::UnboundedSender<AppEvent>>>,
}

impl MacOSMonitor {
    /// Create a new macOS monitor
    pub fn new() -> Result<(Box<dyn OSMonitor>, mpsc::UnboundedReceiver<AppEvent>), Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::unbounded_channel();

        let monitor = MacOSMonitor {
            is_monitoring: Arc::new(Mutex::new(false)),
            event_sender: Arc::new(Mutex::new(tx)),
        };

        Ok((Box::new(monitor), rx))
    }

    /// Check if accessibility permissions are granted
    pub fn check_accessibility_permission() -> bool {
        #[cfg(target_os = "macos")]
        {
            // For now, we'll assume permissions are granted
            // In a production app, you would use AXIsProcessTrusted()
            // which requires linking to ApplicationServices framework

            unsafe {
                // Basic check - can we access NSWorkspace?
                let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
                workspace != nil
            }
        }

        #[cfg(not(target_os = "macos"))]
        false
    }

    /// Get running applications from NSWorkspace
    unsafe fn get_running_applications() -> Vec<AppInfo> {
        let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let apps: id = msg_send![workspace, runningApplications];

        let count: usize = msg_send![apps, count];
        let mut result = Vec::new();

        for i in 0..count {
            let app: id = msg_send![apps, objectAtIndex: i];
            if let Some(app_info) = Self::app_info_from_nsrunningapplication(app) {
                result.push(app_info);
            }
        }

        result
    }

    /// Convert NSRunningApplication to AppInfo
    unsafe fn app_info_from_nsrunningapplication(app: id) -> Option<AppInfo> {
        if app == nil {
            return None;
        }

        // Get localized name
        let name_nsstring: id = msg_send![app, localizedName];
        let name = if name_nsstring != nil {
            let c_str: *const i8 = msg_send![name_nsstring, UTF8String];
            if !c_str.is_null() {
                CStr::from_ptr(c_str).to_string_lossy().to_string()
            } else {
                return None;
            }
        } else {
            return None;
        };

        // Get bundle identifier
        let bundle_id_nsstring: id = msg_send![app, bundleIdentifier];
        let bundle_id = if bundle_id_nsstring != nil {
            let c_str: *const i8 = msg_send![bundle_id_nsstring, UTF8String];
            if !c_str.is_null() {
                CStr::from_ptr(c_str).to_string_lossy().to_string()
            } else {
                return None;
            }
        } else {
            return None;
        };

        // Get process identifier
        let process_id: i32 = msg_send![app, processIdentifier];

        // Get bundle URL for executable path
        let bundle_url: id = msg_send![app, bundleURL];
        let executable_path = if bundle_url != nil {
            let path_nsstring: id = msg_send![bundle_url, path];
            if path_nsstring != nil {
                let c_str: *const i8 = msg_send![path_nsstring, UTF8String];
                if !c_str.is_null() {
                    Some(CStr::from_ptr(c_str).to_string_lossy().to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        Some(AppInfo::with_details(
            name,
            bundle_id,
            process_id as u32,
            None, // version not easily accessible from NSRunningApplication
            executable_path,
        ))
    }
}

impl OSMonitor for MacOSMonitor {
    fn start_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut is_monitoring = self.is_monitoring.lock().unwrap();
        if *is_monitoring {
            return Ok(());
        }

        println!("Starting macOS application monitoring");

        // Note: Full NSWorkspace notification implementation would require:
        // 1. Setting up an observer object with callback methods
        // 2. Registering for notifications using NSNotificationCenter
        // 3. Running an event loop to receive notifications
        //
        // This is a simplified implementation that demonstrates the structure
        // A full implementation would use objc runtime to create observer classes

        if !Self::check_accessibility_permission() {
            eprintln!("Warning: Accessibility permissions may not be granted");
            eprintln!("Some features may be limited");
        }

        *is_monitoring = true;
        Ok(())
    }

    fn stop_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut is_monitoring = self.is_monitoring.lock().unwrap();
        if !*is_monitoring {
            return Ok(());
        }

        println!("Stopping macOS application monitoring");
        *is_monitoring = false;
        Ok(())
    }

    fn get_running_apps(&self) -> Result<Vec<AppInfo>, Box<dyn std::error::Error>> {
        unsafe {
            Ok(Self::get_running_applications())
        }
    }

    fn get_frontmost_app(&self) -> Result<Option<AppInfo>, Box<dyn std::error::Error>> {
        unsafe {
            let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
            let frontmost_app: id = msg_send![workspace, frontmostApplication];

            if frontmost_app == nil {
                return Ok(None);
            }

            Ok(Self::app_info_from_nsrunningapplication(frontmost_app))
        }
    }

    fn is_monitoring(&self) -> bool {
        *self.is_monitoring.lock().unwrap()
    }
}

impl Drop for MacOSMonitor {
    fn drop(&mut self) {
        let _ = self.stop_monitoring();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_monitor() {
        let result = MacOSMonitor::new();
        assert!(result.is_ok());

        let (monitor, _rx) = result.unwrap();
        assert!(!monitor.is_monitoring());
    }

    #[test]
    fn test_start_stop_monitoring() {
        let result = MacOSMonitor::new();
        assert!(result.is_ok());

        let (mut monitor, _rx) = result.unwrap();

        // Start monitoring
        assert!(monitor.start_monitoring().is_ok());
        assert!(monitor.is_monitoring());

        // Stop monitoring
        assert!(monitor.stop_monitoring().is_ok());
        assert!(!monitor.is_monitoring());
    }

    #[test]
    fn test_get_running_apps() {
        let result = MacOSMonitor::new();
        assert!(result.is_ok());

        let (monitor, _rx) = result.unwrap();

        // Get running apps
        let apps = monitor.get_running_apps();
        assert!(apps.is_ok());

        let apps = apps.unwrap();
        println!("Found {} running applications", apps.len());

        // Should have at least some apps running
        assert!(!apps.is_empty(), "Should have at least one app running");

        // Print first few apps for debugging
        for (i, app) in apps.iter().take(5).enumerate() {
            println!("App {}: {} ({})", i + 1, app.name, app.bundle_id);
        }
    }

    #[test]
    fn test_get_frontmost_app() {
        let result = MacOSMonitor::new();
        assert!(result.is_ok());

        let (monitor, _rx) = result.unwrap();

        // Get frontmost app
        let frontmost = monitor.get_frontmost_app();
        assert!(frontmost.is_ok());

        if let Ok(Some(app)) = frontmost {
            println!("Frontmost app: {} ({})", app.name, app.bundle_id);
            assert!(!app.name.is_empty());
            assert!(!app.bundle_id.is_empty());
        }
    }

    #[test]
    fn test_check_accessibility_permission() {
        let has_permission = MacOSMonitor::check_accessibility_permission();
        println!("Accessibility permission: {}", has_permission);
        // Don't assert on this as it depends on system configuration
    }
}
