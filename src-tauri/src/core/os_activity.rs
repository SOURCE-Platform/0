use crate::models::activity::{AppEvent, AppEventType, AppInfo};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

use crate::core::consent::ConsentManager;
use crate::core::database::Database;

// ==============================================================================
// OsMonitor Trait
// ==============================================================================

#[async_trait]
pub trait OsMonitor: Send + Sync {
    async fn start_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    async fn stop_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn subscribe_events(&self) -> mpsc::Receiver<AppEvent>;
    fn get_running_apps(&self) -> Result<Vec<AppInfo>, Box<dyn std::error::Error + Send + Sync>>;
    fn get_frontmost_app(&self) -> Result<Option<AppInfo>, Box<dyn std::error::Error + Send + Sync>>;
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

    fn switch_focus(&mut self, new_pid: u32, app_name: String, bundle_id: String, timestamp: i64) -> Option<FocusDuration> {
        if let Some((old_pid, old_name, old_bundle, start)) = self.current_app.take() {
            let duration_ms = timestamp - start;
            let duration = Duration::from_millis(duration_ms as u64);

            self.focus_history.entry(old_pid)
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
// Activity Storage
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AppUsage {
    pub id: String,
    pub session_id: String,
    pub app_name: String,
    pub bundle_id: String,
    pub process_id: i64,
    pub start_timestamp: i64,
    pub end_timestamp: Option<i64>,
    pub focus_duration_ms: i64,
    pub background_duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AppUsageStats {
    pub app_name: String,
    pub bundle_id: String,
    pub total_focus_duration_ms: i64,
    pub total_background_duration_ms: i64,
    pub launch_count: i64,
    pub first_launch: i64,
    pub last_terminate: Option<i64>,
}

#[derive(Clone)]
pub struct ActivityStorage {
    db: Arc<Database>,
}

impl ActivityStorage {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn init_schema(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let pool = self.db.pool();

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS app_usage (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                app_name TEXT NOT NULL,
                bundle_id TEXT NOT NULL,
                process_id INTEGER NOT NULL,
                start_timestamp INTEGER NOT NULL,
                end_timestamp INTEGER,
                focus_duration_ms INTEGER DEFAULT 0,
                background_duration_ms INTEGER DEFAULT 0,
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            )"
        )
        .execute(pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_app_usage_session ON app_usage(session_id)")
            .execute(pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_app_usage_app ON app_usage(app_name)")
            .execute(pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_app_usage_time ON app_usage(start_timestamp)")
            .execute(pool)
            .await?;

        Ok(())
    }

    pub async fn record_app_launch(&self, session_id: &str, event: AppEvent) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let id = Uuid::new_v4().to_string();

        sqlx::query(
            "INSERT INTO app_usage (id, session_id, app_name, bundle_id, process_id, start_timestamp)
             VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(id)
        .bind(session_id)
        .bind(event.app_info.name)
        .bind(event.app_info.bundle_id)
        .bind(event.app_info.process_id as i64)
        .bind(event.timestamp)
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    pub async fn record_app_terminate(&self, event: AppEvent) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query(
            "UPDATE app_usage SET end_timestamp = ?
             WHERE process_id = ? AND end_timestamp IS NULL"
        )
        .bind(event.timestamp)
        .bind(event.app_info.process_id as i64)
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    pub async fn record_focus_duration(&self, duration: FocusDuration) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query(
            "UPDATE app_usage
             SET focus_duration_ms = focus_duration_ms + ?
             WHERE process_id = ? AND end_timestamp IS NULL"
        )
        .bind(duration.duration_ms)
        .bind(duration.process_id as i64)
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    pub async fn get_app_usage_for_session(&self, session_id: String) -> Result<Vec<AppUsage>, Box<dyn std::error::Error + Send + Sync>> {
        let results = sqlx::query_as::<_, AppUsage>(
            "SELECT id, session_id, app_name, bundle_id, process_id,
                    start_timestamp, end_timestamp, focus_duration_ms, background_duration_ms
             FROM app_usage
             WHERE session_id = ?
             ORDER BY start_timestamp DESC"
        )
        .bind(session_id)
        .fetch_all(self.db.pool())
        .await?;

        Ok(results)
    }

    pub async fn get_app_usage_stats(&self, session_id: String) -> Result<Vec<AppUsageStats>, Box<dyn std::error::Error + Send + Sync>> {
        let results = sqlx::query_as::<_, AppUsageStats>(
            "SELECT
                app_name,
                bundle_id,
                SUM(focus_duration_ms) as total_focus_duration_ms,
                SUM(background_duration_ms) as total_background_duration_ms,
                COUNT(*) as launch_count,
                MIN(start_timestamp) as first_launch,
                MAX(end_timestamp) as last_terminate
             FROM app_usage
             WHERE session_id = ?
             GROUP BY app_name, bundle_id
             ORDER BY total_focus_duration_ms DESC"
        )
        .bind(session_id)
        .fetch_all(self.db.pool())
        .await?;

        Ok(results)
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
    pub async fn new(consent_manager: Arc<ConsentManager>, db: Arc<Database>) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
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

    pub async fn start_recording(&self, session_id: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Check OsActivity consent
        use crate::core::consent::Feature;
        let has_consent = self.consent_manager
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

    pub async fn get_app_usage_stats(&self, session_id: String) -> Result<Vec<AppUsageStats>, Box<dyn std::error::Error + Send + Sync>> {
        self.storage.get_app_usage_stats(session_id).await
    }

    pub async fn get_current_app(&self) -> Result<Option<AppInfo>, Box<dyn std::error::Error + Send + Sync>> {
        let monitor = self.monitor.read().await;
        monitor.get_frontmost_app()
    }

    pub async fn get_running_apps(&self) -> Result<Vec<AppInfo>, Box<dyn std::error::Error + Send + Sync>> {
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
                        if let Err(e) = storage.record_focus_duration(duration).await {
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
