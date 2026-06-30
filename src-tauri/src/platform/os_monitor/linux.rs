// Linux application monitoring (X11 and Wayland)

use crate::models::activity::{AppEvent, AppEventType, AppInfo};
use crate::platform::os_monitor::linux_support::{
    detect_display_server, get_active_window_pid_x11, get_frontmost_app_linux, get_gui_processes,
    get_process_info_from_proc, DisplayServer,
};
use crate::platform::os_monitor::OSMonitor;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Linux application monitor
pub struct LinuxMonitor {
    display_server: DisplayServer,
    is_monitoring: Arc<Mutex<bool>>,
    event_sender: Arc<Mutex<mpsc::UnboundedSender<AppEvent>>>,
    monitoring_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl LinuxMonitor {
    pub fn new(
    ) -> Result<(Box<dyn OSMonitor>, mpsc::UnboundedReceiver<AppEvent>), Box<dyn std::error::Error>>
    {
        let (tx, rx) = mpsc::unbounded_channel();

        // Detect display server
        let display_server = detect_display_server();

        let monitor = LinuxMonitor {
            display_server,
            is_monitoring: Arc::new(Mutex::new(false)),
            event_sender: Arc::new(Mutex::new(tx)),
            monitoring_task: Arc::new(Mutex::new(None)),
        };

        Ok((Box::new(monitor), rx))
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
            let current_processes = get_gui_processes();
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
                if let Some(active_pid) = get_active_window_pid_x11() {
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
                        if let Some(app_info) = get_process_info_from_proc(active_pid as i32) {
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
        Ok(get_gui_processes())
    }

    fn get_frontmost_app(&self) -> Result<Option<AppInfo>, Box<dyn std::error::Error>> {
        Ok(get_frontmost_app_linux())
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
        let display_server = detect_display_server();
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
