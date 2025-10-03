// Linux application monitoring (X11 and Wayland)

use crate::models::activity::{AppEvent, AppEventType, AppInfo};
use crate::platform::os_monitor::OSMonitor;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Display server type
#[derive(Debug, Clone, Copy, PartialEq)]
enum DisplayServer {
    X11,
    Wayland,
    Unknown,
}

/// Linux application monitor
pub struct LinuxMonitor {
    display_server: DisplayServer,
    is_monitoring: Arc<Mutex<bool>>,
    event_sender: Arc<Mutex<mpsc::UnboundedSender<AppEvent>>>,
    monitoring_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl LinuxMonitor {
    pub fn new() -> Result<(Box<dyn OSMonitor>, mpsc::UnboundedReceiver<AppEvent>), Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::unbounded_channel();

        // Detect display server
        let display_server = Self::detect_display_server();

        let monitor = LinuxMonitor {
            display_server,
            is_monitoring: Arc::new(Mutex::new(false)),
            event_sender: Arc::new(Mutex::new(tx)),
            monitoring_task: Arc::new(Mutex::new(None)),
        };

        Ok((Box::new(monitor), rx))
    }

    /// Detect which display server is running
    fn detect_display_server() -> DisplayServer {
        // Check environment variables
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            DisplayServer::Wayland
        } else if std::env::var("DISPLAY").is_ok() {
            DisplayServer::X11
        } else {
            DisplayServer::Unknown
        }
    }

    /// Get process info from /proc filesystem
    fn get_process_info_from_proc(pid: i32) -> Option<AppInfo> {
        match procfs::process::Process::new(pid) {
            Ok(process) => {
                // Get process name
                let name = process.stat.ok()?.comm;

                // Get executable path
                let executable_path = process.exe().ok()?.to_string_lossy().to_string();

                // Use executable path as bundle_id
                let bundle_id = executable_path.clone();

                Some(AppInfo::with_details(
                    name,
                    bundle_id,
                    pid as u32,
                    None,
                    Some(executable_path),
                ))
            }
            Err(_) => None,
        }
    }

    /// Check if a process is a GUI application
    fn is_gui_app(process: &procfs::process::Process) -> bool {
        // Check if process has DISPLAY or WAYLAND_DISPLAY environment variable
        if let Ok(environ) = process.environ() {
            environ.contains_key("DISPLAY") || environ.contains_key("WAYLAND_DISPLAY")
        } else {
            false
        }
    }

    /// Get all running GUI applications
    fn get_gui_processes() -> Vec<AppInfo> {
        let mut apps = Vec::new();

        if let Ok(all_procs) = procfs::process::all_processes() {
            for proc_result in all_procs {
                if let Ok(process) = proc_result {
                    // Only include GUI apps
                    if Self::is_gui_app(&process) {
                        if let Some(app_info) = Self::get_process_info_from_proc(process.pid) {
                            // Filter out some common system processes
                            if !app_info.name.is_empty()
                                && !app_info.name.starts_with("systemd")
                                && !app_info.name.starts_with("dbus")
                            {
                                apps.push(app_info);
                            }
                        }
                    }
                }
            }
        }

        apps
    }

    /// Get active window PID on X11
    #[cfg(target_os = "linux")]
    fn get_active_window_pid_x11() -> Option<u32> {
        use x11::xlib::*;
        use std::ptr;

        unsafe {
            let display = XOpenDisplay(ptr::null());
            if display.is_null() {
                return None;
            }

            let root = XDefaultRootWindow(display);

            // Get _NET_ACTIVE_WINDOW atom
            let active_window_atom =
                XInternAtom(display, b"_NET_ACTIVE_WINDOW\0".as_ptr() as *const i8, 0);

            // Get active window
            let mut actual_type = 0;
            let mut actual_format = 0;
            let mut nitems = 0;
            let mut bytes_after = 0;
            let mut prop: *mut u8 = ptr::null_mut();

            let status = XGetWindowProperty(
                display,
                root,
                active_window_atom,
                0,
                1,
                0,
                0, // AnyPropertyType
                &mut actual_type,
                &mut actual_format,
                &mut nitems,
                &mut bytes_after,
                &mut prop,
            );

            if status != 0 || prop.is_null() || nitems == 0 {
                XCloseDisplay(display);
                return None;
            }

            let window = *(prop as *const u64);
            XFree(prop as *mut _);

            // Get _NET_WM_PID from active window
            let pid_atom = XInternAtom(display, b"_NET_WM_PID\0".as_ptr() as *const i8, 0);

            let status = XGetWindowProperty(
                display,
                window,
                pid_atom,
                0,
                1,
                0,
                6, // XA_CARDINAL
                &mut actual_type,
                &mut actual_format,
                &mut nitems,
                &mut bytes_after,
                &mut prop,
            );

            let pid = if status == 0 && !prop.is_null() && nitems > 0 {
                let pid = *(prop as *const u32);
                XFree(prop as *mut _);
                Some(pid)
            } else {
                None
            };

            XCloseDisplay(display);
            pid
        }
    }

    /// Get frontmost app (best effort on Wayland)
    fn get_frontmost_app_linux() -> Option<AppInfo> {
        let display_server = Self::detect_display_server();

        match display_server {
            DisplayServer::X11 => {
                // Try to get active window PID
                if let Some(pid) = Self::get_active_window_pid_x11() {
                    Self::get_process_info_from_proc(pid as i32)
                } else {
                    None
                }
            }
            DisplayServer::Wayland => {
                // Wayland doesn't expose active window info
                // Best effort: use process with highest CPU usage as proxy
                // For now, just return None
                // TODO: Implement heuristic-based detection
                None
            }
            DisplayServer::Unknown => None,
        }
    }

    /// Background task to monitor process changes
    async fn monitoring_loop(
        is_monitoring: Arc<Mutex<bool>>,
        event_sender: Arc<Mutex<mpsc::UnboundedSender<AppEvent>>>,
        display_server: DisplayServer,
    ) {
        let mut previous_pids = HashSet::new();
        let mut previous_active_pid: Option<u32> = None;

        loop {
            // Check if we should stop
            {
                let monitoring = is_monitoring.lock().unwrap();
                if !*monitoring {
                    break;
                }
            }

            // Get current GUI processes
            let current_processes = Self::get_gui_processes();
            let current_pids: HashSet<u32> =
                current_processes.iter().map(|p| p.process_id).collect();

            // Detect new processes (Launch)
            for pid in current_pids.difference(&previous_pids) {
                if let Some(app_info) = current_processes.iter().find(|p| p.process_id == *pid) {
                    let event = AppEvent {
                        timestamp: chrono::Utc::now().timestamp_millis(),
                        event_type: AppEventType::Launch,
                        app_info: app_info.clone(),
                    };

                    let sender = event_sender.lock().unwrap();
                    let _ = sender.send(event);
                }
            }

            // Detect terminated processes
            for _pid in previous_pids.difference(&current_pids) {
                // Skip terminate events for now (would need to cache app info)
            }

            // Check active window changes (X11 only)
            if display_server == DisplayServer::X11 {
                if let Some(active_pid) = Self::get_active_window_pid_x11() {
                    if Some(active_pid) != previous_active_pid {
                        // Send FocusLoss for previous app
                        if let Some(prev_pid) = previous_active_pid {
                            if let Some(prev_app) =
                                current_processes.iter().find(|p| p.process_id == prev_pid)
                            {
                                let event = AppEvent {
                                    timestamp: chrono::Utc::now().timestamp_millis(),
                                    event_type: AppEventType::FocusLoss,
                                    app_info: prev_app.clone(),
                                };

                                let sender = event_sender.lock().unwrap();
                                let _ = sender.send(event);
                            }
                        }

                        // Send FocusGain for current app
                        if let Some(app_info) = Self::get_process_info_from_proc(active_pid as i32)
                        {
                            let event = AppEvent {
                                timestamp: chrono::Utc::now().timestamp_millis(),
                                event_type: AppEventType::FocusGain,
                                app_info,
                            };

                            let sender = event_sender.lock().unwrap();
                            let _ = sender.send(event);
                        }

                        previous_active_pid = Some(active_pid);
                    }
                }
            }

            previous_pids = current_pids;

            // Poll every second
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }
}

impl OSMonitor for LinuxMonitor {
    fn start_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut is_monitoring = self.is_monitoring.lock().unwrap();
        if *is_monitoring {
            return Ok(());
        }

        println!(
            "Starting Linux application monitoring (Display server: {:?})",
            self.display_server
        );

        if self.display_server == DisplayServer::Wayland {
            println!("Note: Wayland has limited window focus detection capabilities");
        }

        *is_monitoring = true;
        drop(is_monitoring);

        // Start monitoring task
        let is_monitoring_clone = Arc::clone(&self.is_monitoring);
        let event_sender_clone = Arc::clone(&self.event_sender);
        let display_server = self.display_server;

        let task = tokio::spawn(async move {
            Self::monitoring_loop(is_monitoring_clone, event_sender_clone, display_server).await;
        });

        *self.monitoring_task.lock().unwrap() = Some(task);

        Ok(())
    }

    fn stop_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut is_monitoring = self.is_monitoring.lock().unwrap();
        if !*is_monitoring {
            return Ok(());
        }

        println!("Stopping Linux application monitoring");
        *is_monitoring = false;
        drop(is_monitoring);

        // Cancel the monitoring task
        if let Some(task) = self.monitoring_task.lock().unwrap().take() {
            task.abort();
        }

        Ok(())
    }

    fn get_running_apps(&self) -> Result<Vec<AppInfo>, Box<dyn std::error::Error>> {
        Ok(Self::get_gui_processes())
    }

    fn get_frontmost_app(&self) -> Result<Option<AppInfo>, Box<dyn std::error::Error>> {
        Ok(Self::get_frontmost_app_linux())
    }

    fn is_monitoring(&self) -> bool {
        *self.is_monitoring.lock().unwrap()
    }
}

impl Drop for LinuxMonitor {
    fn drop(&mut self) {
        let _ = self.stop_monitoring();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_display_server() {
        let display_server = LinuxMonitor::detect_display_server();
        println!("Detected display server: {:?}", display_server);
        // Don't assert - depends on environment
    }

    #[test]
    fn test_create_monitor() {
        let result = LinuxMonitor::new();
        assert!(result.is_ok());

        let (monitor, _rx) = result.unwrap();
        assert!(!monitor.is_monitoring());
    }

    #[tokio::test]
    async fn test_get_running_apps() {
        let result = LinuxMonitor::new();
        assert!(result.is_ok());

        let (monitor, _rx) = result.unwrap();

        let apps = monitor.get_running_apps();
        assert!(apps.is_ok());

        let apps = apps.unwrap();
        println!("Found {} GUI applications", apps.len());

        for (i, app) in apps.iter().take(5).enumerate() {
            println!("App {}: {} (PID: {})", i + 1, app.name, app.process_id);
        }
    }

    #[tokio::test]
    async fn test_get_frontmost_app() {
        let result = LinuxMonitor::new();
        assert!(result.is_ok());

        let (monitor, _rx) = result.unwrap();

        let frontmost = monitor.get_frontmost_app();
        assert!(frontmost.is_ok());

        match frontmost.unwrap() {
            Some(app) => {
                println!("Frontmost app: {} (PID: {})", app.name, app.process_id);
            }
            None => {
                println!("No frontmost app detected (may be on Wayland)");
            }
        }
    }
}
