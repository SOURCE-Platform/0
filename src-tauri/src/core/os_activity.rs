use crate::models::activity::{AppEvent, AppEventType, AppInfo};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};

use crate::core::consent::ConsentManager;
use crate::core::database::Database;
pub use crate::core::os_activity_storage::{ActivityStorage, AppUsage, AppUsageStats};

// ==============================================================================
// OsMonitor Trait
// ==============================================================================

#[async_trait]
pub trait OsMonitor: Send + Sync {
    async fn start_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    async fn stop_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn subscribe_events(&self) -> mpsc::Receiver<AppEvent>;
    fn get_running_apps(&self) -> Result<Vec<AppInfo>, Box<dyn std::error::Error + Send + Sync>>;
    fn get_frontmost_app(
        &self,
    ) -> Result<Option<AppInfo>, Box<dyn std::error::Error + Send + Sync>>;
}

// ==============================================================================
// Factory Function
// ==============================================================================

pub fn create_os_monitor() -> Result<Box<dyn OsMonitor>, Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(target_os = "macos")]
    {
        use crate::platform::os_monitor::MacOSMonitor;
        return Ok(Box::new(MacOSMonitor::new()?));
    }

    #[cfg(target_os = "windows")]
    {
        use crate::platform::os_monitor::WindowsMonitor;
        return Ok(Box::new(WindowsMonitor::new()?));
    }

    #[cfg(target_os = "linux")]
    {
        use crate::platform::os_monitor::LinuxMonitor;
        return Ok(Box::new(LinuxMonitor::new()?));
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("OS monitoring not supported on this platform".into())
    }
}

// ==============================================================================
// Focus Tracking
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusDuration {
    pub process_id: u32,
    pub app_name: String,
    pub bundle_id: String,
    pub duration_ms: i64,
    pub start_time: i64,
    pub end_time: i64,
}

struct FocusTracker {
    current_app: Option<(u32, String, String, i64)>, // (process_id, app_name, bundle_id, focus_start_time)
    focus_history: HashMap<u32, Duration>,
}

impl FocusTracker {
    fn new() -> Self {
        Self {
            current_app: None,
            focus_history: HashMap::new(),
        }
    }

    fn switch_focus(
        &mut self,
        new_pid: u32,
        app_name: String,
        bundle_id: String,
        timestamp: i64,
    ) -> Option<FocusDuration> {
        if let Some((old_pid, old_name, old_bundle, start)) = self.current_app.take() {
            let duration_ms = timestamp - start;
            let duration = Duration::from_millis(duration_ms as u64);

            self.focus_history
                .entry(old_pid)
                .and_modify(|d| *d += duration)
                .or_insert(duration);

            self.current_app = Some((new_pid, app_name, bundle_id, timestamp));

            return Some(FocusDuration {
                process_id: old_pid,
                app_name: old_name,
                bundle_id: old_bundle,
                duration_ms,
                start_time: start,
                end_time: timestamp,
            });
        }

        self.current_app = Some((new_pid, app_name, bundle_id, timestamp));
        None
    }

    fn remove_app(&mut self, process_id: u32) {
        if let Some((pid, _, _, _)) = self.current_app {
            if pid == process_id {
                self.current_app = None;
            }
        }
        self.focus_history.remove(&process_id);
    }
}

// ==============================================================================
// OS Activity Recorder
// ==============================================================================

pub struct OsActivityRecorder {
    monitor: Arc<RwLock<Box<dyn OsMonitor>>>,
    consent_manager: Arc<ConsentManager>,
    storage: ActivityStorage,
    current_session_id: Arc<RwLock<Option<String>>>,
    is_recording: Arc<RwLock<bool>>,
}

impl OsActivityRecorder {
    pub async fn new(
        consent_manager: Arc<ConsentManager>,
        db: Arc<Database>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let monitor = create_os_monitor()?;
        let storage = ActivityStorage::new(db);
        storage.init_schema().await?;

        Ok(Self {
            monitor: Arc::new(RwLock::new(monitor)),
            consent_manager,
            storage,
            current_session_id: Arc::new(RwLock::new(None)),
            is_recording: Arc::new(RwLock::new(false)),
        })
    }

    pub async fn start_recording(
        &self,
        session_id: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Check OsActivity consent
        use crate::core::consent::Feature;
        let has_consent = self
            .consent_manager
            .is_consent_granted(Feature::OsActivity)
            .await
            .map_err(|e| format!("Consent check failed: {}", e))?;

        if !has_consent {
            return Err("OsActivity consent not granted".into());
        }

        let mut is_recording = self.is_recording.write().await;
        if *is_recording {
            return Err("Already recording".into());
        }

        // Store session ID
        *self.current_session_id.write().await = Some(session_id.clone());

        // Start monitoring
        let mut monitor = self.monitor.write().await;
        monitor.start_monitoring().await?;

        // Subscribe to events
        let event_rx = monitor.subscribe_events();
        drop(monitor);

        *is_recording = true;

        // Spawn background task to process events
        let storage = self.storage.clone();
        let current_session_id = self.current_session_id.clone();
        let is_recording_clone = self.is_recording.clone();

        tokio::spawn(async move {
            Self::process_events(event_rx, storage, current_session_id, is_recording_clone).await;
        });

        Ok(())
    }

    pub async fn stop_recording(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut is_recording = self.is_recording.write().await;
        if !*is_recording {
            return Ok(());
        }

        let mut monitor = self.monitor.write().await;
        monitor.stop_monitoring().await?;

        *is_recording = false;
        *self.current_session_id.write().await = None;

        Ok(())
    }

    pub async fn get_app_usage_stats(
        &self,
        session_id: String,
    ) -> Result<Vec<AppUsageStats>, Box<dyn std::error::Error + Send + Sync>> {
        self.storage.get_app_usage_stats(session_id).await
    }

    pub async fn get_current_app(
        &self,
    ) -> Result<Option<AppInfo>, Box<dyn std::error::Error + Send + Sync>> {
        let monitor = self.monitor.read().await;
        monitor.get_frontmost_app()
    }

    pub async fn get_running_apps(
        &self,
    ) -> Result<Vec<AppInfo>, Box<dyn std::error::Error + Send + Sync>> {
        let monitor = self.monitor.read().await;
        monitor.get_running_apps()
    }

    async fn process_events(
        mut event_rx: mpsc::Receiver<AppEvent>,
        storage: ActivityStorage,
        current_session_id: Arc<RwLock<Option<String>>>,
        is_recording: Arc<RwLock<bool>>,
    ) {
        let mut focus_tracker = FocusTracker::new();

        while let Some(event) = event_rx.recv().await {
            // Check if still recording
            if !*is_recording.read().await {
                break;
            }

            let session_id = match &*current_session_id.read().await {
                Some(id) => id.clone(),
                None => continue,
            };

            match event.event_type {
                AppEventType::Launch => {
                    if let Err(e) = storage.record_app_launch(&session_id, event.clone()).await {
                        eprintln!("Error recording app launch: {}", e);
                    }
                }
                AppEventType::Terminate => {
                    if let Err(e) = storage.record_app_terminate(event.clone()).await {
                        eprintln!("Error recording app terminate: {}", e);
                    }
                    focus_tracker.remove_app(event.app_info.process_id);
                }
                AppEventType::FocusGain => {
                    if let Some(duration) = focus_tracker.switch_focus(
                        event.app_info.process_id,
                        event.app_info.name.clone(),
                        event.app_info.bundle_id.clone(),
                        event.timestamp,
                    ) {
                        if let Err(e) = storage
                            .record_focus_duration(duration.process_id, duration.duration_ms)
                            .await
                        {
                            eprintln!("Error recording focus duration: {}", e);
                        }
                    }
                }
                AppEventType::FocusLoss => {
                    // Tracked by FocusGain of next app
                }
            }
        }
    }
}
