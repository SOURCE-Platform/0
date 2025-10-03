use crate::core::database::Database;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

// ==============================================================================
// Configuration
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub idle_timeout_minutes: u32,           // Default: 30
    pub minimum_session_duration_minutes: u32, // Default: 5
    pub auto_end_on_sleep: bool,             // Default: true
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            idle_timeout_minutes: 30,
            minimum_session_duration_minutes: 5,
            auto_end_on_sleep: true,
        }
    }
}

// ==============================================================================
// Session Types
// ==============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionType {
    Work,
    Communication,
    Research,
    Entertainment,
    Development,
    Unknown,
}

impl SessionType {
    pub fn to_string(&self) -> &'static str {
        match self {
            SessionType::Work => "work",
            SessionType::Communication => "communication",
            SessionType::Research => "research",
            SessionType::Entertainment => "entertainment",
            SessionType::Development => "development",
            SessionType::Unknown => "unknown",
        }
    }

    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "work" => SessionType::Work,
            "communication" => SessionType::Communication,
            "research" => SessionType::Research,
            "entertainment" => SessionType::Entertainment,
            "development" => SessionType::Development,
            _ => SessionType::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub start_timestamp: i64,
    pub end_timestamp: Option<i64>,
    pub session_type: Option<String>,
    pub device_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetrics {
    pub total_duration_ms: u64,
    pub active_duration_ms: u64,
    pub idle_duration_ms: u64,
    pub app_switches: u32,
    pub unique_apps: u32,
    pub most_used_app: String,
    pub productivity_score: f32, // 0.0 - 1.0
}

#[derive(Debug, Clone)]
pub struct AppUsageInfo {
    pub app_name: String,
    pub focus_duration_ms: i64,
}

// ==============================================================================
// Idle Detection
// ==============================================================================

pub struct IdleDetector;

impl IdleDetector {
    pub async fn get_idle_time() -> Result<Duration, Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(target_os = "macos")]
        {
            Self::get_idle_time_macos()
        }

        #[cfg(target_os = "windows")]
        {
            Self::get_idle_time_windows()
        }

        #[cfg(target_os = "linux")]
        {
            Self::get_idle_time_linux()
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            Ok(Duration::from_secs(0))
        }
    }

    #[cfg(target_os = "macos")]
    fn get_idle_time_macos() -> Result<Duration, Box<dyn std::error::Error + Send + Sync>> {
        // For now, return a mock value
        // TODO: Implement using IOKit's kIOHIDIdleTimeKey
        // This requires Core Foundation bindings
        Ok(Duration::from_secs(0))
    }

    #[cfg(target_os = "windows")]
    fn get_idle_time_windows() -> Result<Duration, Box<dyn std::error::Error + Send + Sync>> {
        // TODO: Implement using GetLastInputInfo
        Ok(Duration::from_secs(0))
    }

    #[cfg(target_os = "linux")]
    fn get_idle_time_linux() -> Result<Duration, Box<dyn std::error::Error + Send + Sync>> {
        // TODO: Implement using XScreenSaverQueryInfo or logind
        Ok(Duration::from_secs(0))
    }
}

// ==============================================================================
// Power Event Monitoring
// ==============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    WillSleep,
    DidWake,
    BatteryLow,
}

pub struct PowerEventMonitor {
    event_tx: mpsc::Sender<PowerEvent>,
}

impl PowerEventMonitor {
    pub fn new() -> (Self, mpsc::Receiver<PowerEvent>) {
        let (tx, rx) = mpsc::channel(10);
        (Self { event_tx: tx }, rx)
    }

    pub async fn start_monitoring(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // TODO: Platform-specific implementation
        // macOS: IORegisterForSystemPower
        // Windows: RegisterPowerSettingNotification
        // Linux: D-Bus org.freedesktop.login1.Manager PrepareForSleep
        Ok(())
    }
}

// ==============================================================================
// Session Classification
// ==============================================================================

fn categorize_app(app_name: &str) -> SessionType {
    let name_lower = app_name.to_lowercase();

    if name_lower.contains("code")
        || name_lower.contains("studio")
        || name_lower.contains("vim")
        || name_lower.contains("xcode")
        || name_lower.contains("intellij")
    {
        return SessionType::Development;
    }

    if name_lower.contains("slack")
        || name_lower.contains("teams")
        || name_lower.contains("zoom")
        || name_lower.contains("discord")
        || name_lower.contains("mail")
        || name_lower.contains("outlook")
    {
        return SessionType::Communication;
    }

    if name_lower.contains("safari")
        || name_lower.contains("chrome")
        || name_lower.contains("firefox")
        || name_lower.contains("browser")
    {
        return SessionType::Research;
    }

    if name_lower.contains("spotify")
        || name_lower.contains("netflix")
        || name_lower.contains("youtube")
        || name_lower.contains("music")
        || name_lower.contains("games")
    {
        return SessionType::Entertainment;
    }

    if name_lower.contains("word")
        || name_lower.contains("excel")
        || name_lower.contains("powerpoint")
        || name_lower.contains("keynote")
        || name_lower.contains("pages")
    {
        return SessionType::Work;
    }

    SessionType::Unknown
}

fn calculate_productivity_score(apps: &[AppUsageInfo]) -> f32 {
    if apps.is_empty() {
        return 0.0;
    }

    let total_focus: i64 = apps.iter().map(|a| a.focus_duration_ms).sum();
    if total_focus == 0 {
        return 0.0;
    }

    // Average focus time per app (higher is better - means more sustained focus)
    let avg_focus_per_app = total_focus as f32 / apps.len() as f32;

    // Penalty for too many app switches (context switching is bad for productivity)
    let switch_penalty = 1.0 / (1.0 + apps.len() as f32 * 0.1);

    // Normalize average focus time to minutes, cap at 1.0 for 60+ minutes per app
    let focus_score = (avg_focus_per_app / 60000.0).min(1.0);

    focus_score * switch_penalty
}

// ==============================================================================
// Session Manager
// ==============================================================================

pub struct SessionManager {
    db: Arc<Database>,
    current_session_id: Arc<RwLock<Option<String>>>,
    config: SessionConfig,
    monitoring: Arc<RwLock<bool>>,
}

impl SessionManager {
    pub async fn new(
        db: Arc<Database>,
        config: SessionConfig,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self {
            db,
            current_session_id: Arc::new(RwLock::new(None)),
            config,
            monitoring: Arc::new(RwLock::new(false)),
        })
    }

    pub async fn start_monitoring(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut monitoring = self.monitoring.write().await;
        if *monitoring {
            return Ok(());
        }

        *monitoring = true;

        // Spawn background task to monitor for session start/end
        let db = self.db.clone();
        let current_session_id = self.current_session_id.clone();
        let config = self.config.clone();
        let monitoring_flag = self.monitoring.clone();

        tokio::spawn(async move {
            Self::monitor_loop(db, current_session_id, config, monitoring_flag).await;
        });

        Ok(())
    }

    async fn monitor_loop(
        db: Arc<Database>,
        current_session_id: Arc<RwLock<Option<String>>>,
        config: SessionConfig,
        monitoring: Arc<RwLock<bool>>,
    ) {
        loop {
            // Check if still monitoring
            if !*monitoring.read().await {
                break;
            }

            // Check idle time
            if let Ok(idle_time) = IdleDetector::get_idle_time().await {
                let current_session = current_session_id.read().await.clone();

                if idle_time.as_secs() < 60 {
                    // User is active
                    if current_session.is_none() {
                        // Start new session
                        if let Ok(session_id) = Self::create_session_internal(&db).await {
                            *current_session_id.write().await = Some(session_id);
                            println!("Started new session");
                        }
                    }
                } else if idle_time.as_secs() > config.idle_timeout_minutes as u64 * 60 {
                    // User is idle beyond threshold
                    if let Some(session_id) = current_session {
                        // End current session
                        if let Ok(_) = Self::end_session_internal(&db, &session_id).await {
                            *current_session_id.write().await = None;
                            println!("Ended session due to idle timeout");
                        }
                    }
                }
            }

            // Sleep for 30 seconds before checking again
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    }

    pub async fn stop_monitoring(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        *self.monitoring.write().await = false;
        Ok(())
    }

    pub async fn get_or_create_session(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let current = self.current_session_id.read().await;
        if let Some(session_id) = current.as_ref() {
            return Ok(session_id.clone());
        }
        drop(current);

        // Create new session
        let session_id = Self::create_session_internal(&self.db).await?;
        *self.current_session_id.write().await = Some(session_id.clone());

        Ok(session_id)
    }

    async fn create_session_internal(db: &Arc<Database>) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let session_id = Uuid::new_v4().to_string();
        let start_timestamp = chrono::Utc::now().timestamp_millis();
        let device_id = Self::get_device_id();

        db.create_session(&session_id, start_timestamp, &device_id).await?;

        Ok(session_id)
    }

    pub async fn end_current_session(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let session_id = self.current_session_id.read().await.clone();
        if let Some(id) = session_id {
            Self::end_session_internal(&self.db, &id).await?;
            *self.current_session_id.write().await = None;
        }
        Ok(())
    }

    async fn end_session_internal(db: &Arc<Database>, session_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let end_timestamp = chrono::Utc::now().timestamp_millis();
        db.end_session(session_id, end_timestamp).await?;
        Ok(())
    }

    pub async fn get_current_session(&self) -> Result<Option<Session>, Box<dyn std::error::Error + Send + Sync>> {
        let session_id = self.current_session_id.read().await.clone();
        if let Some(id) = session_id {
            self.get_session_by_id(&id).await.map(Some)
        } else {
            Ok(None)
        }
    }

    pub async fn get_session_by_id(&self, session_id: &str) -> Result<Session, Box<dyn std::error::Error + Send + Sync>> {
        let session = self.db.get_session(session_id).await?;
        Ok(Session {
            id: session.id,
            start_timestamp: session.start_timestamp,
            end_timestamp: session.end_timestamp,
            session_type: None, // Will be calculated on demand
            device_id: session.device_id,
        })
    }

    pub async fn get_sessions_in_range(
        &self,
        start: i64,
        end: i64,
    ) -> Result<Vec<Session>, Box<dyn std::error::Error + Send + Sync>> {
        let db_sessions = self.db.get_sessions_in_range(start, end).await?;

        let sessions = db_sessions
            .into_iter()
            .map(|s| Session {
                id: s.id,
                start_timestamp: s.start_timestamp,
                end_timestamp: s.end_timestamp,
                session_type: None,
                device_id: s.device_id,
            })
            .collect();

        Ok(sessions)
    }

    pub async fn classify_session_type(&self, session_id: &str) -> Result<SessionType, Box<dyn std::error::Error + Send + Sync>> {
        // Get app usage for this session from the app_usage table
        let apps = self.get_app_usage_for_session(session_id).await?;

        if apps.is_empty() {
            return Ok(SessionType::Unknown);
        }

        let mut category_scores: HashMap<SessionType, f32> = HashMap::new();

        for app in apps {
            let category = categorize_app(&app.app_name);
            let score = app.focus_duration_ms as f32 / 1000.0; // Convert to seconds

            *category_scores.entry(category).or_insert(0.0) += score;
        }

        // Return category with highest score
        let session_type = category_scores
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(cat, _)| cat)
            .unwrap_or(SessionType::Unknown);

        Ok(session_type)
    }

    pub async fn calculate_session_metrics(&self, session_id: &str) -> Result<SessionMetrics, Box<dyn std::error::Error + Send + Sync>> {
        let session = self.get_session_by_id(session_id).await?;
        let apps = self.get_app_usage_for_session(session_id).await?;

        let total_duration = if let Some(end) = session.end_timestamp {
            end - session.start_timestamp
        } else {
            chrono::Utc::now().timestamp_millis() - session.start_timestamp
        };

        let active_duration: i64 = apps.iter().map(|a| a.focus_duration_ms).sum();

        let idle_duration = total_duration - active_duration;

        let app_switches = apps.len() as u32;

        let unique_apps = apps
            .iter()
            .map(|a| &a.app_name)
            .collect::<HashSet<_>>()
            .len() as u32;

        let most_used_app = apps
            .iter()
            .max_by_key(|a| a.focus_duration_ms)
            .map(|a| a.app_name.clone())
            .unwrap_or_else(|| "None".to_string());

        let productivity_score = calculate_productivity_score(&apps);

        Ok(SessionMetrics {
            total_duration_ms: total_duration as u64,
            active_duration_ms: active_duration as u64,
            idle_duration_ms: idle_duration.max(0) as u64,
            app_switches,
            unique_apps,
            most_used_app,
            productivity_score,
        })
    }

    async fn get_app_usage_for_session(&self, session_id: &str) -> Result<Vec<AppUsageInfo>, Box<dyn std::error::Error + Send + Sync>> {
        // Query the app_usage table
        #[derive(sqlx::FromRow)]
        struct AppUsageRow {
            app_name: String,
            total_focus: Option<i64>,
        }

        let results = sqlx::query_as::<_, AppUsageRow>(
            "SELECT app_name, SUM(focus_duration_ms) as total_focus
             FROM app_usage
             WHERE session_id = ?
             GROUP BY app_name
             ORDER BY total_focus DESC"
        )
        .bind(session_id)
        .fetch_all(self.db.pool())
        .await?;

        let apps = results
            .into_iter()
            .map(|row| AppUsageInfo {
                app_name: row.app_name,
                focus_duration_ms: row.total_focus.unwrap_or(0),
            })
            .collect();

        Ok(apps)
    }

    fn get_device_id() -> String {
        // Simple device ID based on hostname
        // In production, this should be a persistent UUID stored in config
        hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "unknown".to_string())
    }
}
