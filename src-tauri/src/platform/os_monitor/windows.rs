// Windows application monitoring using Win32 API

use crate::models::activity::{AppEvent, AppEventType, AppInfo};
use crate::platform::os_monitor::OSMonitor;
use std::collections::HashSet;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND, MAX_PATH};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::GetModuleFileNameExW;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// Windows application monitor
pub struct WindowsMonitor {
    is_monitoring: Arc<Mutex<bool>>,
    event_sender: Arc<Mutex<mpsc::UnboundedSender<AppEvent>>>,
    monitoring_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl WindowsMonitor {
    pub fn new() -> Result<(Box<dyn OSMonitor>, mpsc::UnboundedReceiver<AppEvent>), Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::unbounded_channel();

        let monitor = WindowsMonitor {
            is_monitoring: Arc::new(Mutex::new(false)),
            event_sender: Arc::new(Mutex::new(tx)),
            monitoring_task: Arc::new(Mutex::new(None)),
        };

        Ok((Box::new(monitor), rx))
    }

    /// Get process info from process ID
    fn get_process_info(process_id: u32) -> Option<AppInfo> {
        unsafe {
            let handle = match OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
                false,
                process_id,
            ) {
                Ok(h) => h,
                Err(_) => return None,
            };

            // Get executable path
            let mut path_buffer = [0u16; MAX_PATH as usize];
            let len = GetModuleFileNameExW(handle, None, &mut path_buffer);

            let _ = CloseHandle(handle);

            if len == 0 {
                return None;
            }

            let executable_path = OsString::from_wide(&path_buffer[..len as usize])
                .to_string_lossy()
                .to_string();

            // Extract app name from path
            let name = std::path::Path::new(&executable_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string();

            // Use executable path as bundle_id since Windows doesn't have bundle IDs
            let bundle_id = executable_path.clone();

            Some(AppInfo::with_details(
                name,
                bundle_id,
                process_id,
                None, // Version not easily accessible
                Some(executable_path),
            ))
        }
    }

    /// Get all running processes
    fn get_all_processes() -> Vec<AppInfo> {
        unsafe {
            let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                Ok(s) => s,
                Err(_) => return Vec::new(),
            };

            let mut processes = Vec::new();
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };

            if Process32FirstW(snapshot, &mut entry).is_ok() {
                loop {
                    let process_id = entry.th32ProcessID;

                    if let Some(app_info) = Self::get_process_info(process_id) {
                        // Filter out system processes
                        if !app_info.name.is_empty()
                            && process_id > 0
                            && !app_info.name.eq_ignore_ascii_case("System")
                        {
                            processes.push(app_info);
                        }
                    }

                    if Process32NextW(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }

            let _ = CloseHandle(snapshot);
            processes
        }
    }

    /// Get the foreground window's process
    fn get_foreground_process() -> Option<AppInfo> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0 == 0 {
                return None;
            }

            let mut process_id = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut process_id));

            if process_id == 0 {
                return None;
            }

            Self::get_process_info(process_id)
        }
    }

    /// Background task to monitor process and focus changes
    async fn monitoring_loop(
        is_monitoring: Arc<Mutex<bool>>,
        event_sender: Arc<Mutex<mpsc::UnboundedSender<AppEvent>>>,
    ) {
        let mut previous_pids = HashSet::new();
        let mut previous_foreground_pid: Option<u32> = None;

        loop {
            // Check if we should stop
            {
                let monitoring = is_monitoring.lock().unwrap();
                if !*monitoring {
                    break;
                }
            }

            // Get current processes
            let current_processes = Self::get_all_processes();
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
            for pid in previous_pids.difference(&current_pids) {
                // We don't have the app_info anymore, so we'll skip sending Terminate events
                // In a production system, we'd cache the info
            }

            // Check foreground window changes
            if let Some(foreground_app) = Self::get_foreground_process() {
                let current_fg_pid = Some(foreground_app.process_id);

                if current_fg_pid != previous_foreground_pid {
                    // Send FocusLoss for previous app
                    if let Some(prev_pid) = previous_foreground_pid {
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
                    let event = AppEvent {
                        timestamp: chrono::Utc::now().timestamp_millis(),
                        event_type: AppEventType::FocusGain,
                        app_info: foreground_app.clone(),
                    };

                    let sender = event_sender.lock().unwrap();
                    let _ = sender.send(event);

                    previous_foreground_pid = current_fg_pid;
                }
            }

            previous_pids = current_pids;

            // Poll every second
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }
}

impl OSMonitor for WindowsMonitor {
    fn start_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut is_monitoring = self.is_monitoring.lock().unwrap();
        if *is_monitoring {
            return Ok(());
        }

        println!("Starting Windows application monitoring");
        *is_monitoring = true;
        drop(is_monitoring);

        // Start monitoring task
        let is_monitoring_clone = Arc::clone(&self.is_monitoring);
        let event_sender_clone = Arc::clone(&self.event_sender);

        let task = tokio::spawn(async move {
            Self::monitoring_loop(is_monitoring_clone, event_sender_clone).await;
        });

        *self.monitoring_task.lock().unwrap() = Some(task);

        Ok(())
    }

    fn stop_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut is_monitoring = self.is_monitoring.lock().unwrap();
        if !*is_monitoring {
            return Ok(());
        }

        println!("Stopping Windows application monitoring");
        *is_monitoring = false;
        drop(is_monitoring);

        // Cancel the monitoring task
        if let Some(task) = self.monitoring_task.lock().unwrap().take() {
            task.abort();
        }

        Ok(())
    }

    fn get_running_apps(&self) -> Result<Vec<AppInfo>, Box<dyn std::error::Error>> {
        Ok(Self::get_all_processes())
    }

    fn get_frontmost_app(&self) -> Result<Option<AppInfo>, Box<dyn std::error::Error>> {
        Ok(Self::get_foreground_process())
    }

    fn is_monitoring(&self) -> bool {
        *self.is_monitoring.lock().unwrap()
    }
}

impl Drop for WindowsMonitor {
    fn drop(&mut self) {
        let _ = self.stop_monitoring();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_monitor() {
        let result = WindowsMonitor::new();
        assert!(result.is_ok());

        let (monitor, _rx) = result.unwrap();
        assert!(!monitor.is_monitoring());
    }

    #[tokio::test]
    async fn test_get_running_apps() {
        let result = WindowsMonitor::new();
        assert!(result.is_ok());

        let (monitor, _rx) = result.unwrap();

        let apps = monitor.get_running_apps();
        assert!(apps.is_ok());

        let apps = apps.unwrap();
        println!("Found {} running applications", apps.len());
        assert!(!apps.is_empty(), "Should have at least one app running");

        for (i, app) in apps.iter().take(5).enumerate() {
            println!("App {}: {} (PID: {})", i + 1, app.name, app.process_id);
        }
    }

    #[tokio::test]
    async fn test_get_foreground_app() {
        let result = WindowsMonitor::new();
        assert!(result.is_ok());

        let (monitor, _rx) = result.unwrap();

        let frontmost = monitor.get_frontmost_app();
        assert!(frontmost.is_ok());

        if let Ok(Some(app)) = frontmost {
            println!("Foreground app: {} (PID: {})", app.name, app.process_id);
            assert!(!app.name.is_empty());
        }
    }
}
